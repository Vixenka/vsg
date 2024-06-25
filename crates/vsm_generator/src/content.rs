use std::{
    io::{Cursor, Write},
    path::{Path, PathBuf},
    sync::Arc,
};

use flate2::{write::ZlibEncoder, Compression};
use quick_xml::{
    events::{BytesEnd, BytesStart, Event},
    name::QName,
    Reader,
};
use serde::{Deserialize, Serialize};
use tokio::{
    fs,
    io::{AsyncReadExt, AsyncWriteExt},
    task::JoinSet,
};
use walkdir::WalkDir;

use crate::{content::content_variables::ContentVariables, Context};

use self::preliminary_analysis::PreliminaryAnalysisOutput;

pub mod content_variables;
pub mod markdown;
pub mod preliminary_analysis;
pub mod word_counter;

#[derive(Debug, Serialize, Deserialize)]
pub struct ContentCache {}

#[derive(Debug, Default)]
pub struct ContentResult {
    errors: Vec<anyhow::Error>,
    warnings: Vec<anyhow::Error>,
}

impl ContentResult {
    pub fn new() -> Self {
        Self {
            errors: Vec::new(),
            warnings: Vec::new(),
        }
    }

    pub fn push_error(&mut self, error: anyhow::Error) {
        self.errors.push(error);
    }

    pub fn push_warning(&mut self, warning: anyhow::Error) {
        self.warnings.push(warning);
    }

    pub fn errors(&self) -> &[anyhow::Error] {
        &self.errors
    }

    pub fn warnings(&self) -> &[anyhow::Error] {
        &self.warnings
    }
}

pub async fn process_content(context: &Arc<Context>) -> anyhow::Result<ContentResult> {
    let mut set = JoinSet::new();
    for file in collect_files_for_processing(&Path::new(&context.args.project).join("content")) {
        let context = context.clone();
        set.spawn(async move { preliminary_analysis::analyze_file(context, file).await });
    }

    let mut content_result = ContentResult::new();
    let mut preliminary_outputs = Vec::new();
    while let Some(result) = set.join_next().await {
        let result = match result {
            Ok(previous_step) => previous_step,
            Err(error) => {
                content_result.push_error(error.into());
                continue;
            }
        };

        match result {
            Ok(previous_step) => preliminary_outputs.push(Arc::new(previous_step)),
            Err(error) => {
                content_result.push_error(error);
                continue;
            }
        }
    }

    let md_post_list = markdown::create_md_post_list(&preliminary_outputs).await?;
    context
        .md_post_list
        .set(md_post_list)
        .expect("Unable to set md_post_list.");

    let mut set = JoinSet::new();
    for previous_step in &preliminary_outputs {
        let context = context.clone();
        let previous_step = previous_step.clone();
        set.spawn(async move { process_file(context, previous_step).await });
    }

    while let Some(result) = set.join_next().await {
        let result = match result {
            Ok(previous_step) => previous_step,
            Err(error) => {
                content_result.push_error(error.into());
                continue;
            }
        };

        match result {
            Ok(result) => {
                content_result.errors.extend(result.errors);
                content_result.warnings.extend(result.warnings);
            }
            Err(error) => {
                content_result.push_error(error);
                continue;
            }
        }
    }

    Ok(content_result)
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
        } else if c == ' ' || c == '_' {
            result.push('-');
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

async fn process_file(
    context: Arc<Context>,
    previous_step: Arc<PreliminaryAnalysisOutput>,
) -> anyhow::Result<ContentResult> {
    tracing::trace!("Processing file '{}'.", previous_step.path.display());

    let mut result = ContentResult::new();
    let mut variables = previous_step.variables.clone();
    let html = create_html_file(
        &context,
        &previous_step.template_path,
        &mut variables,
        &mut result,
    )
    .await?;

    let mut output_path = Path::new(&context.args.output).join(
        previous_step
            .path
            .strip_prefix(&context.args.project)
            .expect("Unable to strip prefix."),
    );
    output_path.set_extension("html");

    fs::create_dir_all(output_path.parent().unwrap())
        .await
        .expect("Unable to create directory.");
    fs::File::create(&output_path)
        .await
        .expect("Unable to create file.")
        .write_all(html.as_bytes())
        .await
        .expect("Unable to write file.");

    let mut compressed = Vec::new();
    let mut encoder = ZlibEncoder::new(&mut compressed, Compression::best());
    encoder
        .write_all(html.as_bytes())
        .expect("Unable to write to encoder.");

    output_path.set_extension("html.deflate");
    fs::File::create(&output_path)
        .await
        .expect("Unable to create file.")
        .write_all(encoder.finish().expect("Unable to finish encoder."))
        .await
        .expect("Unable to write file.");

    Ok(result)
}

async fn create_html_file(
    context: &Arc<Context>,
    template_path: &Path,
    variables: &mut ContentVariables,
    result: &mut ContentResult,
) -> anyhow::Result<String> {
    let mut file = match tokio::fs::File::open(&template_path).await {
        Ok(file) => file,
        Err(error) => {
            tracing::error!("Unable to open file: {}.", error);
            return Err(error.into());
        }
    };
    let mut buffer = String::new();
    file.read_to_string(&mut buffer)
        .await
        .expect("Unable to read file.");

    let mut reader = Reader::from_reader(Cursor::new(buffer));
    set_reader_position(&mut reader, context, variables, 0, result);

    let mut last_start_position = None;
    let mut last_edited_position = 0;
    let mut buf: Vec<u8> = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let element_name = get_element_name(&e.name(), &mut reader, template_path)?;

                if let Some(template) = context.templates.get(element_name) {
                    let position = reader.buffer_position();
                    let start_position = position - e.len() - 2;
                    reader
                        .get_mut()
                        .get_mut()
                        .replace_range(start_position..position, &template.data);

                    set_reader_position(&mut reader, context, variables, 0, result);
                    continue;
                } else if element_name == "img" {
                    upgrade_image(
                        &e,
                        &mut reader,
                        &mut last_start_position,
                        &mut last_edited_position,
                    );
                }

                last_start_position = Some(reader.buffer_position());
            }
            Ok(Event::End(e)) => {
                let element_name = get_element_name(&e.name(), &mut reader, template_path)?;
                if upgrade_header(
                    &e,
                    element_name,
                    &mut reader,
                    &mut last_start_position,
                    &mut last_edited_position,
                ) {
                    set_reader_position(&mut reader, context, variables, 0, result);
                }
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
    let minified = minify::html::minify(reader.get_ref().get_ref());
    #[cfg(debug_assertions)]
    let minified = reader.into_inner().into_inner();

    Ok(minified)
}

fn set_reader_position(
    reader: &mut Reader<Cursor<String>>,
    context: &Arc<Context>,
    variables: &mut ContentVariables,
    position: usize,
    result: &mut ContentResult,
) {
    let mut cursor = reader.get_mut().clone();
    cursor.set_position(position as u64);

    let len = cursor.get_mut().len();
    variables.apply(cursor.get_mut(), 0..len, context, result);

    *reader = Reader::from_reader(cursor);
    reader.check_end_names(false);
}

fn get_element_name<'a>(
    e: &QName<'a>,
    reader: &mut Reader<Cursor<String>>,
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
    reader: &mut Reader<Cursor<String>>,
    last_start_position: &mut Option<usize>,
    last_edited_position: &mut usize,
) -> bool {
    let mut position = reader.buffer_position();
    if !(element_name.starts_with('h')
        && element_name
            .as_bytes()
            .get(1)
            .map_or(false, |v| v.is_ascii_digit() && *v != b'1'))
        || last_start_position.is_none()
        || position < *last_edited_position + 1
    {
        return false;
    }

    let last_start_position = last_start_position.take().unwrap();
    if !reader.get_ref().get_ref()[last_start_position - 4..last_start_position].starts_with("<h") {
        return false;
    }

    let start_position = last_start_position - e.len() - 2;
    let id = get_id_from_name(&reader.get_mut().get_mut()[last_start_position..position - 5]);

    let new = format!(
        "<{element_name} class=\"header-text\" id=\"{id}\"><a href=\"#{id}\"><span>#</span> "
    );
    reader
        .get_mut()
        .get_mut()
        .replace_range(start_position..last_start_position, &new);

    position += new.len() - 9;

    reader
        .get_mut()
        .get_mut()
        .replace_range(position..position, "</a>");

    *last_edited_position = position + 9;
    true
}

fn upgrade_image(
    e: &BytesStart,
    reader: &mut Reader<Cursor<String>>,
    last_start_position: &mut Option<usize>,
    last_edited_position: &mut usize,
) {
    let mut position = reader.buffer_position();

    tracing::info!(
        "Image: {}",
        &reader.get_ref().get_ref()[last_start_position.unwrap_or_default()..position]
            .contains("webm")
    );
}
