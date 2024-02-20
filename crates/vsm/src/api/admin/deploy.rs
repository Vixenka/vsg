use std::{fs, path::Path, sync::Arc};

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
    Router,
};
use rand::distributions::{Alphanumeric, DistString};

use crate::AppState;

pub struct DeployState {
    key: String,
}

pub fn initialize(router: Router<Arc<AppState>>) -> (DeployState, Router<Arc<AppState>>) {
    (
        DeployState { key: get_key() },
        router.route("/api/admin/deploy/server", post(server)),
    )
}

fn get_key() -> String {
    let path = Path::new("deploy.txt");
    if path.exists() {
        return fs::read_to_string(path).expect("Unable to read deploy file");
    }

    tracing::info!("Deploy key not found, creating new one.");
    let string = Alphanumeric.sample_string(&mut rand::thread_rng(), 256);
    fs::write("deploy.txt", string.clone()).expect("Unable to write deploy file");
    string
}

async fn server(State(state): State<Arc<AppState>>, body: String) -> Response {
    if body != state.api.admin.deploy.key {
        return (StatusCode::FORBIDDEN, "Invalid key").into_response();
    }

    match deploy_server() {
        Ok(()) => {
            tokio::spawn(async {
                tracing::info!("Server deployed, shutting down in 5 seconds");
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                tracing::info!("Shutting down");
                std::process::exit(0);
            });

            (
                StatusCode::OK,
                "Server deployed successfully. Will be restarted on new version in 5 seconds.",
            )
                .into_response()
        }
        Err(e) => {
            tracing::error!("Deploying failed: {:?}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Deploying failed").into_response()
        }
    }
}

fn deploy_server() -> anyhow::Result<()> {
    tracing::info!("Deploying server");

    #[cfg(feature = "deploy")]
    {
        let child = std::process::Command::new("./vsm_updater")
            .stdout(std::process::Stdio::inherit())
            .spawn()?;

        let output = child.wait_with_output()?;

        if !output.status.success() {
            return Err(anyhow::anyhow!(output.status));
        }
    }
    #[cfg(not(feature = "deploy"))]
    {
        tracing::info!("Deploying not enabled due to missing feature flag");
    }

    tracing::info!("Server deployed successfully");
    Ok(())
}
