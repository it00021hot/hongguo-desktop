use axum::http::Uri;

/// Image endpoints observed in this repository's API evidence; no rewrite for arbitrary hosts.
const IMAGE_HOSTS: &[&str] = &[
    "p3-reading-sign.fqnovelpic.com",
    "p3-novel.byteimg.com",
    "p6-novel.byteimg.com",
    "p26.douyinpic.com",
    "p3-sign.douyinpic.com",
];

pub(crate) fn cdn_image_url(source: &str) -> String {
    let network = source.starts_with("//")
        || source
            .get(..5)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("http:"))
        || source
            .get(..6)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("https:"));
    if network && hongguo_common::offline_qa::external_url_forbidden(source) {
        return String::new();
    }
    normalize(source)
}

fn normalize(source: &str) -> String {
    let Ok(uri) = source.parse::<Uri>() else {
        return source.into();
    };
    let known = uri
        .authority()
        .is_some_and(|authority| IMAGE_HOSTS.contains(&authority.as_str()));
    if !known || !matches!(uri.scheme_str(), Some("http" | "https")) {
        return source.into();
    }
    let mut value = source
        .strip_prefix("http://")
        .map_or_else(|| source.to_owned(), |rest| format!("https://{rest}"));
    // Only an unsigned CDN output template is format-selectable. Signed paths are untouched.
    if uri.query().is_none()
        && uri.path().contains("~tplv-")
        && uri.path().to_ascii_lowercase().ends_with(".heic")
    {
        value.truncate(value.len() - 5);
        value.push_str(".jpeg");
    }
    value
}
