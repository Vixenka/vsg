#[macro_use]
extern crate lazy_static;

use axum::Router;
use clap::{command, Parser};
use tower_http::services::ServeDir;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

pub mod static_sites;

#[cfg(debug_assertions)]
lazy_static! {
    static ref HOT_RELOAD: std::sync::Arc<tokio::sync::broadcast::Sender<()>> =
        std::sync::Arc::new(tokio::sync::broadcast::channel(100).0);
}

static HOT_RELOAD_SCRIPT: &str = r#"
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
}

#[tokio::main]
async fn main() {
    //tracing_subscriber::fmt::init();
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "example_websockets=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let args = Args::parse();
    let generator = run_generator(&args);

    let mut app = static_sites::initialize(Router::new())
        .nest_service("/static", ServeDir::new("./output/static"));

    //#[cfg(debug_assertions)]
    //{
    app = app
        .route("/ws/hotreload", axum::routing::get(hot_reload_handler))
        .layer(
            tower_http::trace::TraceLayer::new_for_http().make_span_with(
                tower_http::trace::DefaultMakeSpan::default().include_headers(true),
            ),
        );
    //}

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app.into_make_service())
        .await
        .unwrap();

    #[allow(dropping_copy_types)]
    drop(generator);
}

#[cfg(not(debug_assertions))]
#[allow(clippy::unused_unit)]
fn run_generator(args: &Args) -> () {
    run_generator_impl(args)
}

#[cfg(debug_assertions)]
fn run_generator(args: &Args) -> notify::ReadDirectoryChangesWatcher {
    use notify::Watcher;

    run_generator_impl(args);

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

    let mut command_args = vec!["run", "--bin", "vsm_generator"];
    #[cfg(not(debug_assertions))]
    {
        command_args.push("--release");
    }
    command_args.extend_from_slice(&["--", "--project", &args.project, "--output", &args.output]);

    let output = std::process::Command::new("cargo")
        .args(command_args.as_slice())
        .output()
        .expect("Failed to execute process");

    println!("{}", String::from_utf8_lossy(&output.stdout).trim());
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
