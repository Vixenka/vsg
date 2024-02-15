use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use tokio::{
    fs,
    io::{AsyncReadExt, AsyncWriteExt},
};
use walkdir::WalkDir;

use crate::Context;

pub async fn process_static(context: &Arc<Context>) {
    let tasks: Vec<_> =
        collect_files_for_processing(&Path::new(&context.args.project).join("static"))
            .into_iter()
            .map(|arg| tokio::spawn(process_file(context.clone(), arg)))
            .collect();

    let mut error = false;
    for task in tasks {
        if task.await.unwrap().is_err() {
            error = true;
        }
    }

    if error {
        tracing::error!("Error in processing static files.")
    }
}

fn collect_files_for_processing(path: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();

    for file in WalkDir::new(path).into_iter().filter_map(|file| file.ok()) {
        if !file.file_type().is_file() {
            continue;
        }

        files.push(file.path().to_path_buf());
        tracing::trace!(
            "Added file '{}' to processing tasks.",
            file.path().display()
        );
    }

    files
}

async fn process_file(context: Arc<Context>, path: PathBuf) -> anyhow::Result<()> {
    tracing::trace!("Processing file '{}'.", path.display());

    let mut file = match fs::File::open(&path).await {
        Ok(file) => file,
        Err(error) => {
            tracing::error!("Unable to open file: {}.", error);
            return Err(error.into());
        }
    };
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)
        .await
        .expect("Unable to read file.");

    let output_path = Path::new(&context.args.output).join(
        path.strip_prefix(&context.args.project)
            .expect("Unable to strip prefix."),
    );

    #[cfg(not(debug_assertions))]
    {
        if path
            .extension()
            .map_or(false, |ext| ext == "css" || ext == "js")
        {
            buffer = minify_html::minify(
                buffer.as_slice(),
                &minify_html::Cfg {
                    minify_css: true,
                    minify_js: true,
                    ..minify_html::Cfg::spec_compliant()
                },
            );
        }
    }

    fs::create_dir_all(output_path.parent().unwrap())
        .await
        .expect("Unable to create directory.");
    fs::File::create(output_path)
        .await
        .expect("Unable to create file.")
        .write_all(buffer.as_slice())
        .await
        .expect("Unable to write file.");

    Ok(())
}
