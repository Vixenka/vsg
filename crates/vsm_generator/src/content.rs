use std::{
    io::Cursor,
    ops::Range,
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

pub async fn process_content(context: &Arc<Context>) -> anyhow::Result<()> {
    let mut set = JoinSet::new();
    for file in collect_files_for_processing(&Path::new(&context.args.project).join("content")) {
        let context = context.clone();
        set.spawn(async move { preliminary_analysis::analyze_file(context, file).await });
    }

    let mut preliminary_outputs = Vec::new();
    while let Some(result) = set.join_next().await {
        let result = match result {
            Ok(previous_step) => previous_step,
            Err(error) => {
                tracing::error!("Error in processing content: {}", error);
                continue;
            }
        };

        match result {
            Ok(previous_step) => preliminary_outputs.push(Arc::new(previous_step)),
            Err(error) => {
                tracing::error!("Error in processing content: {}", error);
                continue;
            }
        }
    }

    let md_post_list = preliminary_analysis::create_md_post_list(&preliminary_outputs).await?;
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
                tracing::error!("Error in processing content: {}", error);
                continue;
            }
        };

        match result {
            Ok(_) => {}
            Err(error) => {
                tracing::error!("Error in processing content: {}", error);
                continue;
            }
        }
    }

    Ok(())
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

async fn process_file(
    context: Arc<Context>,
    previous_step: Arc<PreliminaryAnalysisOutput>,
) -> anyhow::Result<()> {
    tracing::trace!("Processing file '{}'.", previous_step.path.display());

    let html = create_html_file(
        &context,
        &previous_step.template_path,
        &previous_step.variables,
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
    fs::File::create(output_path)
        .await
        .expect("Unable to create file.")
        .write_all(html.as_bytes())
        .await
        .expect("Unable to write file.");

    Ok(())
}

async fn create_html_file(
    context: &Arc<Context>,
    template_path: &Path,
    variables: &ContentVariables,
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
    set_reader_position(&mut reader, context, variables, 0)?;

    let mut last_start_position = None;
    let mut last_edited_position = 0;
    let mut buf: Vec<u8> = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                last_start_position = Some(reader.buffer_position());
                let element_name = get_element_name(&e.name(), &mut reader, template_path)?;

                if let Some(template) = context.templates.get(element_name) {
                    let position = reader.buffer_position();
                    let start_position = position - e.len() - 2;
                    reader
                        .get_mut()
                        .get_mut()
                        .replace_range(start_position..position, &template.data);

                    set_reader_position(&mut reader, context, variables, 0)?;
                }
            }
            Ok(Event::End(e)) => {
                let element_name = get_element_name(&e.name(), &mut reader, template_path)?;
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

fn set_reader_position(
    reader: &mut Reader<Cursor<String>>,
    context: &Arc<Context>,
    variables: &ContentVariables,
    position: usize,
) -> anyhow::Result<()> {
    let mut cursor = reader.get_mut().clone();
    cursor.set_position(position as u64);

    let len = cursor.get_mut().len();
    let result = set_variables(cursor.get_mut(), 0..len, context, variables);

    *reader = Reader::from_reader(cursor);
    reader.check_end_names(false);
    result
}

fn set_variables(
    data: &mut String,
    mut range: Range<usize>,
    context: &Arc<Context>,
    variables: &ContentVariables,
) -> anyhow::Result<()> {
    while let Some(start) = data[range.start..range.end].find("{{") {
        range.start += start;
        let end = match data[range.start..range.end].find("}}") {
            Some(end) => range.start + end + 2,
            None => anyhow::bail!(
                "Unable to find end of variable. In position {}.",
                range.start
            ),
        };

        let key = &data[range.start + 2..end - 2];

        let variable_content = match key {
            "md_posts" => context.md_post_list.get().unwrap(),
            _ => match variables.variables.get(key) {
                Some(variable_content) => variable_content,
                None => {
                    range.start += 1;
                    //anyhow::bail!("Unable to find variable with key '{}'", key);
                    continue;
                }
            },
        };

        data.replace_range(range.start..end, variable_content);

        range.end = range.end + variable_content.len() - (end - range.start);
        range.start += variable_content.len();
    }

    Ok(())
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
}
