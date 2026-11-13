use super::{Routes, cancellation::Cancellation, source::Source};
use axum::{
    body::Body,
    extract::{Path, State},
    http::{HeaderMap, Method, StatusCode, header},
    response::{IntoResponse, Response},
};
use hongguo_stream::range::{RangeSpec, parse_range};

const CHUNK_BYTES: u64 = 256 * 1024;

pub(super) async fn serve(
    State(routes): State<Routes>,
    Path(id): Path<String>,
    method: Method,
    headers: HeaderMap,
) -> Response {
    let source = match routes.source(&id, &headers) {
        Ok(source) => source,
        Err(status) => return status.into_response(),
    };
    if source.is_growing() {
        return StatusCode::NOT_FOUND.into_response();
    }
    let Some(total) = source.len() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    bounded(
        source,
        method,
        headers
            .get(header::RANGE)
            .and_then(|value| value.to_str().ok()),
        0,
        total,
    )
}

pub(super) fn bounded(
    source: Source,
    method: Method,
    range: Option<&str>,
    offset: u64,
    total: u64,
) -> Response {
    let spec = if total == 0 {
        RangeSpec::Unsatisfiable
    } else if method == Method::HEAD {
        RangeSpec::Full
    } else {
        parse_range(range, total)
    };
    let (start, end, status) = match spec {
        RangeSpec::Full => (0, total - 1, StatusCode::OK),
        RangeSpec::Closed { start, end } => (start, end, StatusCode::PARTIAL_CONTENT),
        RangeSpec::Open { start } => (start, total - 1, StatusCode::PARTIAL_CONTENT),
        RangeSpec::Unsatisfiable => {
            let mut response = StatusCode::RANGE_NOT_SATISFIABLE.into_response();
            response.headers_mut().insert(
                header::CONTENT_RANGE,
                format!("bytes */{total}").parse().expect("numeric range"),
            );
            return response;
        }
    };
    let body = if method == Method::HEAD {
        Body::empty()
    } else {
        let Some(first) = offset.checked_add(start) else {
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        };
        let Some(last) = offset.checked_add(end).and_then(|end| end.checked_add(1)) else {
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        };
        stream(source, first, last)
    };
    let mut response = Response::new(body);
    *response.status_mut() = status;
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        "video/mp4".parse().expect("static media type"),
    );
    headers.insert(
        header::ACCEPT_RANGES,
        "bytes".parse().expect("static range unit"),
    );
    headers.insert(
        header::CACHE_CONTROL,
        "no-store".parse().expect("static cache policy"),
    );
    headers.insert(header::CONTENT_LENGTH, (end - start + 1).into());
    if status == StatusCode::PARTIAL_CONTENT {
        headers.insert(
            header::CONTENT_RANGE,
            format!("bytes {start}-{end}/{total}")
                .parse()
                .expect("numeric range"),
        );
    }
    response
}

fn stream(source: Source, start: u64, end: u64) -> Body {
    let guard = Cancellation::default();
    Body::from_stream(futures_util::stream::try_unfold(
        (source, start, guard),
        move |(source, at, guard)| async move {
            if at >= end {
                return Ok::<_, std::io::Error>(None);
            }
            let count = CHUNK_BYTES.min(end - at);
            let reader = source.clone();
            let cancelled = guard.flag();
            let bytes = hongguo_service::runtime::spawn_blocking(move || {
                reader.read(at, count, &cancelled)
            })
            .await
            .map_err(std::io::Error::other)?
            .map_err(std::io::Error::other)?;
            if bytes.is_empty() {
                return Err(std::io::Error::from(std::io::ErrorKind::UnexpectedEof));
            }
            let next = at + bytes.len() as u64;
            Ok(Some((bytes, (source, next, guard))))
        },
    ))
}
