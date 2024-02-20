use std::sync::Arc;

use axum::{
    body::Body,
    extract::{Path, Request, State},
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};

use tokio::fs;

use crate::{analytics, helper, AppState};

pub fn initialize(router: Router<Arc<AppState>>) -> Router<Arc<AppState>> {
    router.route("/", get(root)).route("/*path", get(tree))
}

async fn root(State(state): State<Arc<AppState>>, request: Request<Body>) -> Response {
    serve_impl(state, "index".to_owned(), request).await
}

async fn tree(
    State(state): State<Arc<AppState>>,
    Path(path): Path<String>,
    request: Request<Body>,
) -> Response {
    serve_impl(state, path, request).await
}

async fn serve_impl(state: Arc<AppState>, path: String, request: Request<Body>) -> Response {
    let mut file_path = std::path::Path::new("./output/content").join(&path);
    if file_path.extension().is_some() {
        return error_404(&path);
    }

    let accept_gzip = helper::accept_gzip(&request);
    file_path.set_extension(match accept_gzip {
        true => "html.deflate",
        false => "html",
    });

    let file_content = fs::read(file_path);

    let path_clone = path.clone();
    tokio::spawn(async move { analytics::push(state, path_clone, request).await });

    match file_content.await {
        #[allow(unused_mut)]
        Ok(mut content) => {
            #[cfg(debug_assertions)]
            content.extend_from_slice(crate::HOT_RELOAD_SCRIPT);
            serve_data(accept_gzip, content)
        }
        Err(_) => error_404(&path),
    }
}

fn serve_data(accept_gzip: bool, content: Vec<u8>) -> Response {
    match accept_gzip {
        true => (
            StatusCode::OK,
            [
                (
                    header::CONTENT_TYPE,
                    HeaderValue::from_static("text/html; charset=utf-8"),
                ),
                (
                    header::CONTENT_ENCODING,
                    HeaderValue::from_static("deflate"),
                ),
            ],
            content,
        )
            .into_response(),
        false => (
            StatusCode::OK,
            [(
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/html; charset=utf-8"),
            )],
            content,
        )
            .into_response(),
    }
}

fn error_404(path: &str) -> Response {
    (StatusCode::NOT_FOUND, format!("Not found {path}")).into_response()
}
