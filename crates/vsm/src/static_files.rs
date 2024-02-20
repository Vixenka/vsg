use std::sync::Arc;

use axum::{
    body::Body,
    extract::{Path, Request, State},
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};

use mime_guess::mime;
use tokio::fs;

use crate::{analytics, helper, AppState};

pub fn initialize(router: Router<Arc<AppState>>) -> Router<Arc<AppState>> {
    router.route("/static/*path", get(serve))
}

async fn serve(
    State(state): State<Arc<AppState>>,
    Path(path): Path<String>,
    request: Request<Body>,
) -> Response {
    let mut file_path = std::path::Path::new("./output/static").join(&path);

    let mime = match mime_guess::from_path(&file_path).first() {
        Some(mime) => mime,
        None => mime::TEXT_PLAIN,
    }
    .essence_str()
    .to_owned();

    let accept_gzip = helper::accept_gzip_include_mime(&mime, &request);
    if accept_gzip {
        file_path.set_extension(format!(
            "{}.deflate",
            file_path
                .extension()
                .map_or("", |ext| ext.to_str().unwrap())
        ));
    }

    let file_content = fs::read(&file_path);

    tokio::spawn(analytics::push(state, path.clone(), request));

    match file_content.await {
        #[allow(unused_mut)]
        Ok(mut content) => serve_data(accept_gzip, content, &mime),
        Err(_) => error_404(&path),
    }
}

fn serve_data(accept_gzip: bool, content: Vec<u8>, mime: &str) -> Response {
    let content_type =
        HeaderValue::from_str(mime).unwrap_or(HeaderValue::from_static("text/plain"));

    match accept_gzip {
        true => (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, content_type),
                (
                    header::CONTENT_ENCODING,
                    HeaderValue::from_static("deflate"),
                ),
                (header::EXPIRES, HeaderValue::from_static("86400")),
            ],
            content,
        )
            .into_response(),
        false => (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, content_type),
                (header::EXPIRES, HeaderValue::from_static("86400")),
            ],
            content,
        )
            .into_response(),
    }
}

fn error_404(path: &str) -> Response {
    (StatusCode::NOT_FOUND, format!("Not found {path}")).into_response()
}
