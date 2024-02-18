use std::{
    io::Cursor,
    path::{Path, PathBuf},
    sync::Arc,
};

use quick_xml::{
    events::{BytesEnd, Event},
    name::QName,
    Reader,
};
use serde::{Deserialize, Serialize};
use tokio::{
    fs,
    io::{AsyncReadExt, AsyncWriteExt},
};
use walkdir::WalkDir;

use crate::{content::content_variables::ContentVariables, Context};

pub mod content_variables;
pub mod markdown;

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

pub fn get_id_from_name(name: &str) -> String {
    let mut name = name;
    if name
        .as_bytes()
        .first()
        .map_or(false, |v| v.is_ascii_digit())
    {
        name = &name[1..];
    }

    if name.starts_with('.') {
        name = &name[1..];
    }

    let mut result = String::new();
    for c in name.trim().chars() {
        if c.is_ascii_alphanumeric() {
            result.push(c.to_ascii_lowercase());
        } else if c == ' ' || c == '-' {
            result.push('_');
        }
    }

    result
}

fn collect_files_for_processing(path: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();

    for file in WalkDir::new(path).into_iter().filter_map(|file| file.ok()) {
        if !file.file_type().is_file() {
            continue;
        }

        let extension = file.path().extension().to_owned().unwrap().to_str();
        if extension != Some("html") && extension != Some("md") {
            continue;
        }

        if file.file_name() == "_template.html" {
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

    let mut variables = ContentVariables::new();
    let template_path = match path.extension().expect("Unable to get extension").to_str() {
        Some("md") => {
            let set_variable = markdown::set_variable(&path, &mut variables);
            let template_path = markdown::get_template(&context, &path);

            set_variable.await?;
            template_path.await?
        }
        _ => path.clone(),
    };

    let html = create_html_file(&context, template_path, &variables).await?;

    let mut output_path = Path::new(&context.args.output).join(
        path.strip_prefix(&context.args.project)
            .expect("Unable to strip prefix."),
    );
    output_path.set_extension("html");

    fs::create_dir_all(output_path.parent().unwrap())
        .await
        .expect("Unable to create directory.");
    fs::File::create(output_path)
        .await
        .expect("Unable to create file.")
        .write_all(html.as_slice())
        .await
        .expect("Unable to write file.");

    Ok(())
}

async fn create_html_file(
    context: &Arc<Context>,
    template_path: PathBuf,
    variables: &ContentVariables,
) -> anyhow::Result<Vec<u8>> {
    let mut file = match tokio::fs::File::open(&template_path).await {
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

    let mut last_start_position = None;
    let mut last_edited_position = 0;
    let mut buf: Vec<u8> = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                last_start_position = Some(reader.buffer_position());
                let element_name = get_element_name(&e.name(), &mut reader, &template_path)?;

                /*for attribute in e.attributes() {
                    let Ok(attribute) = attribute else {
                        continue;
                    };

                    let text = std::str::from_utf8(&attribute.value)?;
                    for (key, value) in &variables.variables {
                        if text.contains(&format!("{{{{{}}}}}", key)) {
                            let position = reader.buffer_position() - text.len() + attribute.value;
                            reader.get_mut().get_mut().splice(
                                position..(position + key.len() + 4),
                                value.iter().cloned(),
                            );

                            set_reader_position(&mut reader, 0);
                            break;
                        }
                    }
                }*/

                if let Some(template) = context.templates.get(element_name) {
                    let position = reader.buffer_position();
                    let start_position = position - e.len() - 2;
                    reader
                        .get_mut()
                        .get_mut()
                        .splice(start_position..position, template.data.iter().cloned());
                }
            }
            Ok(Event::Text(text)) => {
                let text = std::str::from_utf8(&text)?;
                for (key, value) in &variables.variables {
                    if let Some(position) = text.find(&format!("{{{{{}}}}}", key)) {
                        let position = reader.buffer_position() - text.len() + position;
                        reader
                            .get_mut()
                            .get_mut()
                            .splice(position..(position + key.len() + 4), value.iter().cloned());

                        set_reader_position(&mut reader, 0);
                        break;
                    }
                }
            }
            Ok(Event::End(e)) => {
                let element_name = get_element_name(&e.name(), &mut reader, &template_path)?;
                upgrade_header(
                    &e,
                    element_name,
                    &mut reader,
                    &mut last_start_position,
                    &mut last_edited_position,
                );
            }
            Ok(Event::Eof) => break,
            Err(error) => {
                tracing::error!(
                    "Error in processing file `{}` at position {}: {:?}",
                    template_path.display(),
                    reader.buffer_position(),
                    error,
                );
                return Err(error.into());
            }
            _ => (),
        }

        buf.clear();
    }

    #[cfg(not(debug_assertions))]
    let minified = minify_html::minify(
        reader.get_ref().get_ref().as_slice(),
        &minify_html::Cfg::spec_compliant(),
    );
    #[cfg(debug_assertions)]
    let minified = reader.into_inner().into_inner();

    Ok(minified)
}

fn set_reader_position(reader: &mut Reader<Cursor<Vec<u8>>>, position: usize) {
    let cursor = reader.get_mut();
    cursor.set_position(position as u64);
    *reader = Reader::from_reader(cursor.clone());
    reader.check_end_names(false);
}

fn get_element_name<'a>(
    e: &QName<'a>,
    reader: &mut Reader<Cursor<Vec<u8>>>,
    template_path: &Path,
) -> anyhow::Result<&'a str> {
    match std::str::from_utf8(e.0) {
        Ok(e) => Ok(e),
        Err(error) => {
            tracing::error!(
                "Error in processing file `{}` at position {}: {:?}",
                template_path.display(),
                reader.buffer_position(),
                error
            );
            Err(error.into())
        }
    }
}

fn upgrade_header(
    e: &BytesEnd,
    element_name: &str,
    reader: &mut Reader<Cursor<Vec<u8>>>,
    last_start_position: &mut Option<usize>,
    last_edited_position: &mut usize,
) {
    let mut position = reader.buffer_position();
    if !(element_name.starts_with('h')
        && element_name
            .as_bytes()
            .get(1)
            .map_or(false, |v| v.is_ascii_digit() && *v != b'1'))
        || last_start_position.is_none()
        || position < *last_edited_position + 1
    {
        return;
    }
    let last_start_position = last_start_position.take().unwrap();

    let start_position = last_start_position - e.len() - 2;
    let id = get_id_from_name(
        std::str::from_utf8(&reader.get_mut().get_mut()[last_start_position..position - 5])
            .unwrap(),
    );

    let new = format!(
        "<{element_name} class=\"header-text\" id=\"{id}\"><a href=\"#{id}\"><span>#</span> "
    );
    reader.get_mut().get_mut().splice(
        start_position..last_start_position,
        new.as_bytes().iter().copied(),
    );

    position += new.len() - 9;

    reader
        .get_mut()
        .get_mut()
        .splice(position..position, b"</a>".iter().copied());

    *last_edited_position = position + 9;
}
