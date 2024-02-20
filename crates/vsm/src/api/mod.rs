use std::sync::Arc;

use axum::Router;

use crate::AppState;

pub mod admin;

pub struct ApiState {
    pub admin: admin::AdminState,
}

pub fn initialize(router: Router<Arc<AppState>>) -> (ApiState, Router<Arc<AppState>>) {
    let a = admin::initialize(router);
    (ApiState { admin: a.0 }, a.1)
}
