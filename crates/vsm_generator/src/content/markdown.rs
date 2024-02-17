use std::path::{Path, PathBuf};

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
    let file_content = fs::read_to_string(path).await?;
    let parser = Parser::new(file_content.as_str());

    let mut html = String::new();
    html::push_html(&mut html, parser);

    let cite_notes = generate_cite_notes(&mut html).await;
    let table_of_contents = generate_table_of_contents(&html).await;

    variables.insert("md_content".to_owned(), html.into_bytes());
    variables.insert("md_cite_notes".to_owned(), cite_notes.into_bytes());
    variables.insert(
        "md_table_of_contents".to_owned(),
        table_of_contents.into_bytes(),
    );
    Ok(())
}

async fn generate_table_of_contents(html: &str) -> String {
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

    if table_of_contents.is_empty() {
        table_of_contents.push_str("Unfortunatelly, there are no headers in this article :(");
    } else {
        table_of_contents.insert_str(0, "<li><a class=\"top\" href=\"#\">(Top)</a></li>");
        table_of_contents.insert_str(
            table_of_contents.len() - 5,
            "<li><a href=\"#references\">References</a></li>",
        );
    }

    table_of_contents
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
