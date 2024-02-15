use std::{
    fs::{self, File},
    io::{Cursor, Write},
    path::{Path, PathBuf},
    sync::Arc,
};

use quick_xml::{events::Event, Reader};
use serde::{Deserialize, Serialize};
use tokio::io::AsyncReadExt;
use walkdir::WalkDir;

use crate::Context;

#[derive(Debug, Serialize, Deserialize)]
pub struct ContentCache {}

pub async fn process_content(context: &Arc<Context>) {
    let tasks: Vec<_> =
        collect_files_for_processing(&Path::new(&context.args.project).join("content"))
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
        tracing::error!("Error in processing content.")
    }
}

fn collect_files_for_processing(path: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();

    for file in WalkDir::new(path).into_iter().filter_map(|file| file.ok()) {
        if !file.file_type().is_file()
            || file.path().extension().to_owned().unwrap().to_str() != Some("html")
        {
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

    let mut file = match tokio::fs::File::open(&path).await {
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

    let mut reader = Reader::from_reader(Cursor::new(buffer));
    reader.check_end_names(false);

    let mut buf: Vec<u8> = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let element_name = match std::str::from_utf8(e.name().0) {
                    Ok(e) => e,
                    Err(error) => {
                        tracing::error!(
                            "Error in processing file `{}` at position {}: {:?}",
                            path.display(),
                            reader.buffer_position(),
                            error
                        );
                        return Err(error.into());
                    }
                };

                if let Some(template) = context.templates.get(element_name) {
                    let position = reader.buffer_position();
                    reader.get_mut().get_mut().splice(
                        (position - e.len() - 2)..position,
                        template.data.iter().cloned(),
                    );
                }
            }
            Ok(Event::Eof) => break,
            Err(error) => {
                tracing::error!(
                    "Error in processing file `{}` at position {}: {:?}",
                    path.display(),
                    reader.buffer_position(),
                    error
                );
                return Err(error.into());
            }
            _ => (),
        }

        buf.clear();
    }

    let mut output_path = Path::new(&context.args.output)
        .join("content")
        .join(path.file_name().unwrap());
    output_path.set_extension("html");

    #[cfg(not(debug_assertions))]
    let minified = minify_html::minify(
        reader.get_ref().get_ref().as_slice(),
        &minify_html::Cfg::spec_compliant(),
    );
    #[cfg(debug_assertions)]
    let minified = reader.into_inner().into_inner();

    fs::create_dir_all(output_path.parent().unwrap()).expect("Unable to create directory.");
    File::create(output_path)
        .expect("Unable to create file.")
        .write_all(minified.as_slice())
        .expect("Unable to write file.");

    Ok(())
}
