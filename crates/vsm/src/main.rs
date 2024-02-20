#[cfg(debug_assertions)]
#[macro_use]
extern crate lazy_static;

use std::{net::SocketAddr, process::Stdio, sync::Arc};

use api::ApiState;
use axum::Router;
use clap::{command, Parser};
use database::Database;

pub mod analytics;
pub mod api;
pub mod database;
pub mod helper;
pub mod static_files;
pub mod static_sites;

#[cfg(debug_assertions)]
lazy_static! {
    static ref HOT_RELOAD: std::sync::Arc<tokio::sync::broadcast::Sender<()>> =
        std::sync::Arc::new(tokio::sync::broadcast::channel(100).0);
}

#[cfg(debug_assertions)]
static HOT_RELOAD_SCRIPT: &[u8] = br#"
    <script>
        function hotreload() {
            let socket = new WebSocket("ws://localhost:3000/ws/hotreload");

            socket.addEventListener("open", (event) => {
                console.log("Connected to hot reload web socket.");
            });
            
            socket.addEventListener("message", (event) => {
                console.log("Hot reload triggered.");
                location.reload();
            });

            socket.addEventListener("close", (event) => {
                console.log("Hot reload web socket closed.");
                setTimeout(hotreload, 1000);
            });

            socket.addEventListener("error", (event) => {
                console.error("Hot reload web socket error:", event);
                socket.close();
            });
        }

        hotreload();
    </script>"#;

#[derive(Parser, Debug, Clone)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    /// Path to the project directory
    #[arg(short, long, default_value = "../vixenka.com")]
    project: String,
    /// Path to the output directory
    #[arg(short, long, default_value = "./output")]
    output: String,
    // Page port
    #[arg(long, default_value = "3000")]
    port: u16,
}

pub struct AppState {
    pub args: Args,
    pub database: Database,
    pub api: ApiState,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let args = Args::parse();
    let generator = tokio::spawn(run_generator(args.clone()));

    let database = Database::open(&args)
        .await
        .expect("Failed to open database.");

    let router = static_files::initialize(static_sites::initialize(Router::new()));
    let (api, router) = api::initialize(router);

    #[allow(unused_mut)]
    let mut router = router.with_state(Arc::new(AppState {
        args: args.clone(),
        database,
        api,
    }));

    #[cfg(debug_assertions)]
    {
        router = router.route("/ws/hotreload", axum::routing::get(hot_reload_handler))
    }

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", args.port))
        .await
        .expect("Unable to start listener.");
    tracing::info!("Serve website on http://localhost:{}", args.port);

    axum::serve(listener, router.into_make_service())
        .await
        .expect("Unable to start server.");

    #[allow(dropping_copy_types)]
    drop(generator.await);
}

#[cfg(not(debug_assertions))]
#[allow(clippy::unused_unit)]
async fn run_generator(args: Args) -> () {
    run_generator_impl(&args)
}

#[cfg(debug_assertions)]
async fn run_generator(args: Args) -> notify::ReadDirectoryChangesWatcher {
    use notify::Watcher;

    run_generator_impl(&args);

    let mut watcher = notify::recommended_watcher(|res| match res {
        Ok(_) => {
            let args = Args::parse();
            run_generator_impl(&args);
            _ = HOT_RELOAD.send(());
        }
        Err(e) => tracing::error!("Watch error: {:?}", e),
    })
    .unwrap();
    watcher
        .watch(
            std::path::Path::new(&args.project),
            notify::RecursiveMode::Recursive,
        )
        .unwrap();
    watcher
}

fn run_generator_impl(args: &Args) {
    tracing::info!("Running generator for project: {}", args.project);

    let executable;
    let mut command_args = Vec::new();
    #[cfg(not(feature = "deploy"))]
    {
        executable = "cargo";
        command_args.extend_from_slice(&["run", "--bin", "vsm_generator"]);

        #[cfg(not(debug_assertions))]
        {
            command_args.push("--release");
        }

        command_args.push("--");
    }
    #[cfg(feature = "deploy")]
    {
        executable = "./vsm_generator";
    }
    command_args.extend_from_slice(&["--project", &args.project, "--output", &args.output]);

    let child = std::process::Command::new(executable)
        .args(command_args.as_slice())
        .stdout(Stdio::inherit())
        .spawn()
        .expect("Failed to execute process");

    let output = child
        .wait_with_output()
        .expect("Failed to wait for process");

    if !output.status.success() {
        tracing::error!("Generator failed: {:?}", output.status);
    }
}

#[cfg(debug_assertions)]
async fn hot_reload_handler(
    ws: axum::extract::WebSocketUpgrade,
) -> impl axum::response::IntoResponse {
    ws.on_upgrade(|mut socket| async move {
        tracing::info!("Hot reload web socket connection established.");

        let mut rx = HOT_RELOAD.subscribe();
        while rx.recv().await.is_ok() {
            if let Err(err) = socket
                .send(axum::extract::ws::Message::Text("Reload".to_owned()))
                .await
            {
                tracing::error!("Error sending message: {}", err);
                break;
            }
        }

        tracing::info!("Hot reload web socket connection closed.");
    })
}
