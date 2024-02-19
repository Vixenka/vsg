use axum::{body::Body, extract::Request};

pub fn accept_gzip(request: &Request<Body>) -> bool {
    match request.headers().get("Accept-Encoding") {
        Some(header) => match header.to_str() {
            Ok(str) => str.starts_with("gzip"),
            Err(_) => false,
        },
        None => false,
    }
}

pub fn accept_gzip_include_mime(mime: &str, request: &Request<Body>) -> bool {
    if (mime.starts_with("img/") && mime != "image/svg+xml") || mime.starts_with("image") {
        return false;
    }

    accept_gzip(request)
}
