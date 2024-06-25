use std::{collections::HashMap, path::Path, sync::Arc};

use anyhow::Ok;
use chrono::{DateTime, Utc};
use pulldown_cmark::{html, Parser};
use tokio::fs;
use url::Url;

use crate::Context;

use super::{
    content_variables::ContentVariables,
    preliminary_analysis::{self, Content, PreliminaryAnalysisOutput},
    word_counter,
};

#[derive(Debug, Default)]
pub struct BlogContent {
    pub link: String,
    pub title: String,
    pub description: String,
    pub tags: Vec<String>,
    pub date: DateTime<Utc>,
    pub draft: bool,
    pub technical: bool,
    pub difficulty: f64,
}

#[derive(Debug, Default)]
pub struct ExpContent {}

fn get_element<'a>(
    key: &str,
    md_variables: &'a HashMap<String, VariableValue>,
) -> anyhow::Result<&'a VariableValue> {
    match md_variables.get(key) {
        Some(value) => Ok(value),
        None => anyhow::bail!("Unable to find variable with key '{}'", key),
    }
}

fn get_element_string(
    key: &str,
    md_variables: &HashMap<String, VariableValue>,
) -> anyhow::Result<String> {
    match get_element(key, md_variables)? {
        VariableValue::String(str) => Ok(str.clone()),
        _ => anyhow::bail!("Variable '{}' is not a string.", key),
    }
}

fn get_element_string_vec(
    key: &str,
    md_variables: &HashMap<String, VariableValue>,
) -> anyhow::Result<Vec<String>> {
    match get_element(key, md_variables)? {
        VariableValue::Array(array) => {
            let mut result = Vec::new();
            for value in array {
                if let VariableValue::String(str) = value {
                    result.push(str.clone());
                } else {
                    anyhow::bail!("Variable '{}' is not an array of strings.", key);
                }
            }

            Ok(result)
        }
        _ => anyhow::bail!("Variable '{}' is not an array.", key),
    }
}

fn get_element_date(
    key: &str,
    md_variables: &HashMap<String, VariableValue>,
) -> anyhow::Result<DateTime<Utc>> {
    match get_element(key, md_variables)? {
        VariableValue::Date(date) => Ok(*date),
        _ => anyhow::bail!("Variable '{}' is not a date.", key),
    }
}

fn get_element_bool(
    key: &str,
    md_variables: &HashMap<String, VariableValue>,
) -> anyhow::Result<bool> {
    match get_element(key, md_variables)? {
        VariableValue::Bool(bool) => Ok(*bool),
        _ => anyhow::bail!("Variable '{}' is not a boolean.", key),
    }
}

fn get_element_number(
    key: &str,
    md_variables: &HashMap<String, VariableValue>,
) -> anyhow::Result<f64> {
    match get_element(key, md_variables)? {
        VariableValue::Number(number) => Ok(*number),
        _ => anyhow::bail!("Variable '{}' is not a number.", key),
    }
}

pub async fn set_variables(
    context: &Arc<Context>,
    path: &Path,
    variables: &mut ContentVariables,
) -> anyhow::Result<Content> {
    let mut file_content = fs::read_to_string(path).await?;
    let md_variables = extract_variables(&mut file_content)?;
    let process_variables = process_variables(context, path, variables, md_variables);

    let parser = Parser::new(file_content.as_str());

    let mut html = String::new();
    html::push_html(&mut html, parser);

    let cite_notes = generate_cite_notes(&mut html).await;
    let table_of_contents = preliminary_analysis::generate_table_of_contents(&html, true).await;

    let mut content = process_variables.await?;
    word_counter::compute_read_time(&file_content, &mut content, variables);

    variables.insert("md_content".to_owned(), html);
    variables.insert("md_cite_notes".to_owned(), cite_notes);
    variables.insert(
        "md_table_of_contents_desktop".to_owned(),
        table_of_contents.0,
    );
    variables.insert(
        "md_table_of_contents_mobile".to_owned(),
        table_of_contents.1,
    );

    if let Content::Blog(content) = &content {
        let mut tags = String::new();
        for tag in &content.tags {
            tags.push_str(format!("<a>#<strong>{}</strong></a>", tag).as_str());
        }
        variables.insert("md_tags".to_owned(), tags);
        variables.insert("warning".to_owned(), get_draft_info(content));
    }

    Ok(content)
}

fn get_draft_info(content: &BlogContent) -> String {
    match content.draft {
        true => {
            "<p class=\"warning\">This article is still a draft, changes may occur and should not be taken seriously</p>"
                .to_owned()
        }
        false => "".to_owned(),
    }
}

fn extract_variables(file_content: &mut String) -> anyhow::Result<HashMap<String, VariableValue>> {
    const VARIABLE_KEY: &str = "---";

    let Some(start) = file_content.find(VARIABLE_KEY) else {
        return Ok(HashMap::default());
    };

    let start_with_key = start + VARIABLE_KEY.len();
    let Some(end) = file_content[start_with_key..].find(VARIABLE_KEY) else {
        return Ok(HashMap::default());
    };

    let mut result = HashMap::new();

    let variable_text = &file_content[start_with_key..start_with_key + end];
    for line in variable_text.lines() {
        let mut parts = line.splitn(2, ':');
        if let (Some(key), Some(value)) = (parts.next(), parts.next()) {
            let value = VariableValue::from_str(value)?;
            result.insert(key.trim().to_owned(), value);
        }
    }

    file_content.replace_range(start..start_with_key + end + VARIABLE_KEY.len(), "");
    Ok(result)
}

async fn process_variables(
    context: &Arc<Context>,
    path: &Path,
    variables: &mut ContentVariables,
    md_variables: HashMap<String, VariableValue>,
) -> anyhow::Result<Content> {
    for key in ["title", "description", "date", "background_url"] {
        let Some(value) = md_variables.get(key) else {
            continue;
        };
        let key = format!("md_{}", key);

        match value {
            VariableValue::String(str) => variables.insert(key, str.to_owned()),
            VariableValue::Array(_array) => {
                tracing::warn!("Array variables are not supported yet.");
            }
            VariableValue::Date(date) => variables.insert(
                key,
                format!(
                    r#"{}<div class="tooltip">{}</div>"#,
                    date.format("%e %B %Y"),
                    date.format("%A, %e %B %Y %H:%M:%S UTC")
                ),
            ),
            VariableValue::Bool(bool) => variables.insert(key, bool.to_string()),
            VariableValue::Number(number) => variables.insert(key, number.to_string()),
        };
    }

    let p = path.to_str().unwrap();
    if p.contains("blog/") {
        Ok(Content::Blog(BlogContent {
            link: context.get_file_link(path),
            title: get_element_string("title", &md_variables)?,
            description: get_element_string("description", &md_variables)?,
            tags: get_element_string_vec("tags", &md_variables)?,
            date: get_element_date("date", &md_variables)?,
            draft: get_element_bool("draft", &md_variables)?,
            technical: get_element_bool("technical", &md_variables)?,
            difficulty: get_element_number("difficulty", &md_variables)?,
        }))
    } else if p.contains("exp/") {
        Ok(Content::Exp(ExpContent {}))
    } else {
        anyhow::bail!(
            "Unable to determine content type for path '{}'",
            path.display()
        );
    }
}

async fn generate_cite_notes(html: &mut String) -> String {
    const CITE_NOTE: &str = "[_cn ";

    let mut cite_note_html = String::new();
    let mut cite_note_id = 0;

    let mut index = 0;
    while let Some(position) = html[index..].find(CITE_NOTE) {
        index += position;

        let index_with_cite = index + CITE_NOTE.len();
        match html[index_with_cite..].find(')') {
            Some(end) => {
                cite_note_id += 1;
                if let Err(err) = generate_cite_note_link(
                    &mut cite_note_html,
                    &html[index_with_cite..index_with_cite + end],
                    cite_note_id,
                ) {
                    index += 1;
                    tracing::error!("Unable to generate cite note link: {}", err);
                    continue;
                }

                html.replace_range(
                    index..index + end + CITE_NOTE.len() + 1,
                    format!("<a href=\"#cite-note-{cite_note_id}\" class=\"cite-note\"><sup>[{cite_note_id}]</sup></a>").as_str(),
                );
            }
            None => {
                index += 1;
                tracing::error!("Unable to find closing bracket for cite note.")
            }
        }
    }

    if cite_note_html.is_empty() {
        cite_note_html.push_str("Unfortunatelly, there are no references in this article :(")
    }

    cite_note_html
}

fn generate_cite_note_link(
    cite_note_html: &mut String,
    html: &str,
    cite_note_id: usize,
) -> anyhow::Result<()> {
    let mut bracket_index = match html.find("](") {
        Some(end) => end,
        None => anyhow::bail!("Unable to find opening bracket for cite note."),
    };

    cite_note_html.push_str(format!("<li id=\"cite-note-{cite_note_id}\">").as_str());

    if let Some(description) = get_description_of_cite_note(html, bracket_index) {
        cite_note_html.push_str(description);
        cite_note_html.push_str(" - ");
    }

    bracket_index += 2;
    let mut link = None;
    while bracket_index < html.len() {
        if let Some(link) = link {
            let parsed_url = Url::parse(link)?;
            let host_name = format!("{}", parsed_url.host().expect("Unable to get host name."));
            let host_name = host_name.trim_start_matches("www.");
            cite_note_html.push_str(format!("<a href=\"{link}\">{host_name}</a>").as_str());
        }

        let end = html[bracket_index..]
            .find(' ')
            .unwrap_or(html.len() - bracket_index);

        link = Some(&html[bracket_index..bracket_index + end]);
        bracket_index += end + 1;
    }

    if let Some(link) = link {
        if !link.starts_with("https://web.archive.org/") {
            tracing::error!("Cite note do not have link for 'web.archive.org'.");
        }

        cite_note_html.push_str(format!(" - <a href=\"{link}\">archive</a>").as_str());
    } else {
        tracing::error!("Cite note do not have any link.");
    }

    cite_note_html.push_str("</li>");

    Ok(())
}

fn get_description_of_cite_note(html: &str, bracket_index: usize) -> Option<&str> {
    let trimmed = html[..bracket_index].trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

#[derive(Debug)]
enum VariableValue {
    String(String),
    Bool(bool),
    Number(f64),
    Array(Vec<VariableValue>),
    Date(DateTime<Utc>),
}

impl VariableValue {
    fn from_str(value: &str) -> anyhow::Result<Self> {
        let value = value.trim();
        if value.starts_with('[') {
            let mut array = Vec::new();
            for value in value[1..value.len() - 1].split(',') {
                array.push(Self::from_str(value)?);
            }

            if !value.ends_with(']') {
                tracing::warn!("Array variable is not closed with ']' character.");
            }

            Ok(VariableValue::Array(array))
        } else if value.starts_with('"') {
            if !value.ends_with('"') {
                tracing::warn!("String variable is not closed with '\"' character.");
            }

            Ok(VariableValue::String(
                value[1..value.len() - 1].trim().to_owned(),
            ))
        } else if value == "true" {
            Ok(VariableValue::Bool(true))
        } else if value == "false" {
            Ok(VariableValue::Bool(false))
        } else if let std::result::Result::Ok(value) = value.parse::<f64>() {
            Ok(VariableValue::Number(value))
        } else {
            let date = value.parse::<DateTime<Utc>>()?;
            Ok(VariableValue::Date(date))
        }
    }
}

pub async fn create_md_post_list(
    outputs: &[Arc<PreliminaryAnalysisOutput>],
) -> anyhow::Result<String> {
    let mut result = String::new();

    let mut vec = outputs
        .iter()
        .filter_map(|v| match &v.content {
            Some(Content::Blog(c)) => Some(c),
            _ => None,
        })
        .filter(|v| !v.draft)
        .collect::<Vec<_>>();
    vec.sort_by(|a, b| b.date.cmp(&a.date));

    for content in vec {
        result.push_str(
            format!(
                r#"<div class="post-list">
                    <div class="post-list-top">
                        <a href="{}">{}</a>
                        <div class="tooltip-wrapper">
                            {}
                            <div class="tooltip">{}</div>
                        </div>
                    </div>
                    <p>{}</p>
                    <div class="post-list-tags">"#,
                content.link,
                content.title,
                content.date.format("%e&nbsp;%B&nbsp;%Y"),
                content.date.format("%A, %e %B %Y %H:%M:%S UTC"),
                content.description
            )
            .as_str(),
        );

        for tag in &content.tags {
            result.push_str(format!("<a>#{}</a>", tag).as_str());
        }

        result.push_str("</div></div>");
    }

    if result.is_empty() {
        result.push_str("<p>Unfortunately, page still don't have any posts :(</p>");
    }

    Ok(result)
}
