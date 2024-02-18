use std::{
    collections::HashMap,
    fmt,
    path::{Path, PathBuf},
};

use chrono::{DateTime, Utc};
use pulldown_cmark::{html, Parser};
use tokio::fs;
use url::Url;

use crate::{content, Context};

use super::content_variables::ContentVariables;

pub async fn get_template(context: &Context, path: &Path) -> anyhow::Result<PathBuf> {
    let mut template_path = path.to_path_buf();
    let mut template_found = false;

    let project_directory = Path::new(&context.args.project);
    while let Some(parent) = template_path.parent() {
        if template_path == project_directory {
            break;
        }

        let template = parent.join("_template.html");
        if template.exists() {
            template_path = template;
            template_found = true;
            break;
        }

        template_path = parent.to_path_buf();
    }

    if !template_found {
        return Err(anyhow::anyhow!(
            "Template not found for file '{}'.",
            path.display()
        ));
    }

    Ok(template_path)
}

pub async fn set_variable(path: &Path, variables: &mut ContentVariables) -> anyhow::Result<()> {
    let mut file_content = fs::read_to_string(path).await?;
    let md_variables = extract_variables(&mut file_content)?;
    let process_variables = process_variables(variables, md_variables);

    let parser = Parser::new(file_content.as_str());

    let mut html = String::new();
    html::push_html(&mut html, parser);

    let cite_notes = generate_cite_notes(&mut html).await;
    let table_of_contents = generate_table_of_contents(&html).await;

    process_variables.await;
    variables.insert("md_content".to_owned(), html.into_bytes());
    variables.insert("md_cite_notes".to_owned(), cite_notes.into_bytes());
    variables.insert(
        "md_table_of_contents_desktop".to_owned(),
        table_of_contents.0.into_bytes(),
    );
    variables.insert(
        "md_table_of_contents_mobile".to_owned(),
        table_of_contents.1.into_bytes(),
    );
    Ok(())
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
            let value = VariableValue::from_str(value);
            result.insert(key.trim().to_owned(), value);
        }
    }

    file_content.replace_range(start..start_with_key + end + VARIABLE_KEY.len(), "");
    Ok(result)
}

async fn process_variables(
    variables: &mut ContentVariables,
    md_variables: HashMap<String, VariableValue>,
) {
    for (key, value) in md_variables {
        match value {
            VariableValue::String(str) => variables.insert(key, str.into_bytes()),
            VariableValue::Array(_array) => {
                tracing::warn!("Array variables are not supported yet.");
            }
            VariableValue::Date(date) => variables.insert(
                key,
                format!(
                    r#"{}<div class="tooltip">{}</div>"#,
                    date.format("%e %B %Y"),
                    date.format("%A, %e %B %Y %H:%M:%S UTC")
                )
                .into_bytes(),
            ),
        };
    }
}

async fn generate_table_of_contents(html: &str) -> (String, String) {
    let mut table_of_contents = String::new();
    let mut index = 0;

    let mut last_level = 2;
    let mut header = None;

    while let Some(position) = html[index..].find("<h") {
        let level = html[index + position + 2..index + position + 3]
            .parse::<usize>()
            .unwrap();

        index += position + 4;
        if level == 1 {
            continue;
        }

        match html[index..].find("</h") {
            Some(end) => {
                generate_element_for_table_of_contents(
                    &mut table_of_contents,
                    header,
                    level,
                    last_level,
                );

                last_level = level;
                header = Some(&html[index..index + end]);
                index += end;
            }
            None => {
                index += 1;
                tracing::error!("Unable to find closing bracket for header.");
            }
        }
    }

    generate_element_for_table_of_contents(&mut table_of_contents, header, 2, last_level);

    let is_empty = table_of_contents.is_empty();
    if is_empty {
        table_of_contents.push_str("Unfortunatelly, there are no headers in this article :(");
    } else {
        table_of_contents.push_str("<li><a href=\"#references\">References</a></li>");
    }

    let mut desktop_table_of_contents = table_of_contents.clone();
    if !is_empty {
        desktop_table_of_contents.insert_str(0, "<li><a class=\"top\" href=\"#\">(Top)</a></li>");
    }

    (desktop_table_of_contents, table_of_contents)
}

fn generate_element_for_table_of_contents(
    table_of_contents: &mut String,
    header: Option<&str>,
    level: usize,
    last_level: usize,
) {
    if header.is_none() {
        return;
    }
    let header = header.unwrap();

    let id = content::get_id_from_name(header);

    table_of_contents.push_str("<li>");
    if last_level < level {
        table_of_contents.push_str("<details open><summary>");
    }

    table_of_contents.push_str(format!("<a href=\"#{id}\">{header}</a>").as_str());

    if last_level < level {
        table_of_contents.push_str("</summary><ul>");
    }

    for _ in level..last_level {
        table_of_contents.push_str("</li></ul></details>");
    }

    if last_level >= level {
        table_of_contents.push_str("</li>");
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

enum VariableValue {
    String(String),
    Array(Vec<VariableValue>),
    Date(DateTime<Utc>),
}

impl VariableValue {
    fn from_str(value: &str) -> Self {
        let value = value.trim();
        if value.starts_with('[') {
            let mut array = Vec::new();
            for value in value[1..value.len() - 1].split(',') {
                array.push(Self::from_str(value));
            }

            if !value.ends_with(']') {
                tracing::warn!("Array variable is not closed with ']' character.");
            }

            VariableValue::Array(array)
        } else if value.starts_with('"') {
            if !value.ends_with('"') {
                tracing::warn!("String variable is not closed with '\"' character.");
            }

            VariableValue::String(value[1..value.len() - 1].trim().to_owned())
        } else {
            let date = value.parse::<DateTime<Utc>>().unwrap();
            VariableValue::Date(date)
        }
    }
}

impl fmt::Display for VariableValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VariableValue::String(value) => write!(f, "{}", value),
            VariableValue::Array(array) => {
                write!(f, "[")?;
                for (index, value) in array.iter().enumerate() {
                    if index > 0 {
                        write!(f, ", ")?;
                    }

                    write!(f, "{}", value)?;
                }
                write!(f, "]")
            }
            VariableValue::Date(date) => write!(f, "{}", date.to_rfc3339()),
        }
    }
}
