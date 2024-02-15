use axum::{
    extract::Path,
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::get,
    Router,
};
use tokio::fs;

pub fn initialize(router: Router) -> Router {
    router.route("/", get(root)).route("/*path", get(tree))
}

async fn root() -> impl IntoResponse {
    serve_impl("index").await
}

async fn tree(Path(path): Path<String>) -> impl IntoResponse {
    serve_impl(path.as_str()).await
}

async fn serve_impl(path: &str) -> impl IntoResponse {
    let mut file_path = std::path::Path::new("./output/content").join(path);
    file_path.set_extension("html");
    let file_content = fs::read_to_string(file_path);

    match file_content.await {
        Ok(mut content) => {
            #[cfg(debug_assertions)]
            content.push_str(crate::HOT_RELOAD_SCRIPT);
            Html(content).into_response()
        }
        Err(_) => (StatusCode::NOT_FOUND, format!("Not found {path}")).into_response(),
    }
}
