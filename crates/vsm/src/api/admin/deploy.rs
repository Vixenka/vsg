use std::sync::Arc;

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
    Router,
};

use crate::AppState;

pub struct DeployState {}

pub fn initialize(router: Router<Arc<AppState>>) -> (DeployState, Router<Arc<AppState>>) {
    (
        DeployState {},
        router.route("/api/admin/deploy/server", get(server)),
    )
}

async fn server() -> Response {
    (StatusCode::OK, "what").into_response()
}
