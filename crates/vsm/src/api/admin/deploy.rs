use std::{
    fs,
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

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
    site_deployed: AtomicBool,
    server_deployed: AtomicBool,
}

pub fn initialize(router: Router<Arc<AppState>>) -> (DeployState, Router<Arc<AppState>>) {
    (
        DeployState {
            key: get_key(),
            site_deployed: AtomicBool::new(false),
            server_deployed: AtomicBool::new(false),
        },
        router
            .route("/api/admin/deploy/site", post(site))
            .route("/api/admin/deploy/server", post(server)),
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

async fn site(State(state): State<Arc<AppState>>, body: String) -> Response {
    if body != state.api.admin.deploy.key {
        return (StatusCode::FORBIDDEN, "Invalid key").into_response();
    }

    if state
        .api
        .admin
        .deploy
        .site_deployed
        .swap(true, Ordering::Relaxed)
    {
        return (StatusCode::OK, "Site already deploying").into_response();
    }

    match deploy_site(&state) {
        Ok(()) => {
            #[cfg(not(debug_assertions))]
            tokio::spawn(async move {
                crate::run_generator(state.args.clone()).await;
                state
                    .api
                    .admin
                    .deploy
                    .site_deployed
                    .store(false, Ordering::Relaxed);
            });
            #[cfg(debug_assertions)]
            {
                state
                    .api
                    .admin
                    .deploy
                    .site_deployed
                    .store(false, Ordering::Relaxed);
            }

            (
                StatusCode::OK,
                "Site deployed successfully. Running generator.",
            )
                .into_response()
        }
        Err(e) => {
            state
                .api
                .admin
                .deploy
                .site_deployed
                .store(false, Ordering::Relaxed);

            tracing::error!("Site deploying failed: {:?}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Site deploying failed").into_response()
        }
    }
}

fn deploy_site(state: &Arc<AppState>) -> anyhow::Result<()> {
    tracing::info!("Deploying site");

    let child = std::process::Command::new("git")
        .args(vec!["pull", "origin", "master"])
        .current_dir(Path::new(&state.args.project))
        .stdout(std::process::Stdio::inherit())
        .spawn()?;

    let output = child.wait_with_output()?;

    if !output.status.success() {
        return Err(anyhow::anyhow!(output.status));
    }

    tracing::info!("Site files deployed successfully");
    Ok(())
}

async fn server(State(state): State<Arc<AppState>>, body: String) -> Response {
    if body != state.api.admin.deploy.key {
        return (StatusCode::FORBIDDEN, "Invalid key").into_response();
    }

    if state
        .api
        .admin
        .deploy
        .server_deployed
        .swap(true, Ordering::Relaxed)
    {
        return (StatusCode::OK, "Server already deploying").into_response();
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
            state
                .api
                .admin
                .deploy
                .server_deployed
                .store(false, Ordering::Relaxed);

            tracing::error!("Server deploying failed: {:?}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Server deploying failed").into_response()
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
