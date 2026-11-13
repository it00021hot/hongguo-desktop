use super::{
    Routes,
    cancellation::Cancellation,
    hls_source::{Failure, Published, TARGET_DURATION_SECONDS},
    response,
    source::Source,
};
use axum::{
    body::Body,
    extract::{Path, State},
    http::{HeaderMap, Method, StatusCode, header},
    response::{IntoResponse, Response},
};
use hongguo_media::mp4::fragment_index::ByteSpan;

pub(super) async fn playlist(
    State(routes): State<Routes>,
    Path(id): Path<String>,
    method: Method,
    headers: HeaderMap,
) -> Response {
    let source = match routes.source(&id, &headers) {
        Ok(source) => source,
        Err(status) => return status.into_response(),
    };
    if !source.is_growing() {
        return StatusCode::NOT_FOUND.into_response();
    }
    let published = match load(source, true).await {
        Ok(published) => published,
        Err(error) => return failed(error),
    };
    if published.index.fragments().is_empty() {
        return (
            StatusCode::RANGE_NOT_SATISFIABLE,
            [(header::CONTENT_RANGE, "bytes */0")],
        )
            .into_response();
    }
    let mut text = format!(
        "#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-PLAYLIST-TYPE:EVENT\n#EXT-X-TARGETDURATION:{TARGET_DURATION_SECONDS}\n#EXT-X-MEDIA-SEQUENCE:0\n#EXT-X-MAP:URI=\"init.mp4\"\n"
    );
    for (index, fragment) in published.index.fragments().iter().enumerate() {
        text.push_str(&format!(
            "#EXTINF:{:.6},\nsegment/{index}\n",
            fragment.duration_seconds()
        ));
    }
    if published.completed {
        text.push_str("#EXT-X-ENDLIST\n");
    }
    let len = text.len();
    let body = if method == Method::HEAD {
        Body::empty()
    } else {
        Body::from(text)
    };
    let mut response = Response::new(body);
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        "application/vnd.apple.mpegurl"
            .parse()
            .expect("HLS MIME type"),
    );
    response
        .headers_mut()
        .insert(header::CONTENT_LENGTH, len.into());
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        "no-store".parse().expect("cache policy"),
    );
    response
}

pub(super) async fn initialization(
    State(routes): State<Routes>,
    Path(id): Path<String>,
    method: Method,
    headers: HeaderMap,
) -> Response {
    asset(routes, id, None, method, headers).await
}

pub(super) async fn segment(
    State(routes): State<Routes>,
    Path((id, index)): Path<(String, usize)>,
    method: Method,
    headers: HeaderMap,
) -> Response {
    asset(routes, id, Some(index), method, headers).await
}

async fn asset(
    routes: Routes,
    id: String,
    index: Option<usize>,
    method: Method,
    headers: HeaderMap,
) -> Response {
    let source = match routes.source(&id, &headers) {
        Ok(source) => source,
        Err(status) => return status.into_response(),
    };
    if !source.is_growing() {
        return StatusCode::NOT_FOUND.into_response();
    }
    let published = match load(source.clone(), false).await {
        Ok(published) => published,
        Err(error) => return failed(error),
    };
    let (source, span) = match index {
        Some(index) => (
            source,
            published
                .index
                .fragments()
                .get(index)
                .map(|fragment| fragment.bytes),
        ),
        None => {
            let Some(bytes) = published.index.hls_initialization() else {
                return StatusCode::NOT_FOUND.into_response();
            };
            let len = bytes.len() as u64;
            (source.with_memory(bytes), Some(ByteSpan { offset: 0, len }))
        }
    };
    let Some(ByteSpan { offset, len }) = span else {
        return StatusCode::NOT_FOUND.into_response();
    };
    response::bounded(
        source,
        method,
        headers
            .get(header::RANGE)
            .and_then(|value| value.to_str().ok()),
        offset,
        len,
    )
}

async fn load(source: Source, wait: bool) -> Result<Published, Failure> {
    let guard = Cancellation::default();
    let flag = guard.flag();
    let result = hongguo_service::runtime::spawn_blocking(move || source.hls_snapshot(&flag, wait))
        .await
        .map_err(|error| Failure::Invalid(error.to_string()))?;
    drop(guard);
    result
}

fn failed(error: Failure) -> Response {
    match &error {
        Failure::Cancelled => log::debug!("[NSH media] HLS request cancelled"),
        Failure::Failed(_) => log::warn!("[NSH media] HLS request failed: producer failed"),
        Failure::Invalid(_) => log::warn!("[NSH media] HLS request failed: {error}"),
    }
    let status = match error {
        Failure::Cancelled => StatusCode::CONFLICT,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (status, error.to_string()).into_response()
}
