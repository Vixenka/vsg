pub mod cache;
pub mod content;
pub mod static_files;
pub mod template;
pub mod template_repository;

use std::{
    path::{Path, PathBuf},
    sync::{Arc, OnceLock},
};

use clap::Parser;
use template_repository::TemplateRepository;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path to the project directory
    #[arg(short, long, default_value = "../vixenka.com")]
    project: String,
    /// Path to the output directory
    #[arg(short, long, default_value = "./output")]
    output: String,
}

impl Args {
    pub fn project_content(&self) -> PathBuf {
        Path::new(&self.project).join("content")
    }
}

#[derive(Debug)]
pub struct Context {
    templates: TemplateRepository,
    args: Args,
    md_post_list: OnceLock<String>,
}

impl Context {
    pub fn get_file_link(&self, path: &Path) -> String {
        let mut p = path
            .strip_prefix(&self.args.project_content())
            .expect("Unable to strip prefix")
            .to_owned();
        p.set_extension("");
        p.to_str().expect("Unable to convert to str").to_owned()
    }
}

#[tokio::main]
async fn main() {
    #[cfg(debug_assertions)]
    let mut logger;
    #[cfg(not(debug_assertions))]
    let logger;

    logger = tracing_subscriber::fmt();
    #[cfg(debug_assertions)]
    {
        logger = logger.with_max_level(tracing::Level::TRACE);
    }
    logger.init();

    let args = Args::parse();
    /*let cache = Cache::load_or_new(
        PathBuf::from(&args.project)
            .join(".cache")
            .join("cache.bin"),
    )
    .unwrap();*/

    let templates = match TemplateRepository::load(Path::new(&args.project)) {
        Ok(templates) => templates,
        Err(err) => {
            tracing::error!("Failed to load templates: {}", err);
            return;
        }
    };

    let context = Arc::new(Context {
        templates,
        args,
        md_post_list: OnceLock::new(),
    });
    let result = tokio::join!(
        content::process_content(&context),
        static_files::process_static(&context)
    );

    if let Err(err) = result.0 {
        tracing::error!("Failed to process content: {}", err);
        return;
    }

    tracing::info!("Generated website.")
}
