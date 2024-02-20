use std::sync::Arc;

use axum::Router;

use crate::AppState;

pub mod deploy;

pub struct AdminState {
    pub deploy: deploy::DeployState,
}

pub fn initialize(router: Router<Arc<AppState>>) -> (AdminState, Router<Arc<AppState>>) {
    let a = deploy::initialize(router);
    (AdminState { deploy: a.0 }, a.1)
}
