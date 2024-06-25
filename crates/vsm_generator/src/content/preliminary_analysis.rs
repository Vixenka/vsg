use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use crate::Context;

use super::{
    content_variables::ContentVariables,
    markdown::{self, BlogContent, ExpContent},
};

pub struct PreliminaryAnalysisOutput {
    pub path: PathBuf,
    pub template_path: PathBuf,
    pub variables: ContentVariables,
    pub content: Option<Content>,
}

pub enum Content {
    Blog(BlogContent),
    Exp(ExpContent),
}

pub async fn analyze_file(
    context: Arc<Context>,
    path: PathBuf,
) -> anyhow::Result<PreliminaryAnalysisOutput> {
    tracing::info!("Analyzing file '{}'", path.display());

    let mut variables = ContentVariables::new();
    variables.insert("link".to_owned(), context.get_file_link(&path));

    let mut content = None;
    let extension = path.extension().expect("Unable to get extension").to_str();
    let template_path = match extension {
        Some("md") => {
            let set_variable = markdown::set_variables(&context, &path, &mut variables);
            let template_path = get_template(&context, &path);

            content = Some(set_variable.await?);
            template_path.await?
        }
        _ => path.clone(),
    };

    Ok(PreliminaryAnalysisOutput {
        path,
        template_path,
        variables,
        content,
    })
}

async fn get_template(context: &Context, path: &Path) -> anyhow::Result<PathBuf> {
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
            dbg!(template_path.display());
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

pub async fn generate_table_of_contents(html: &str, link_references: bool) -> (String, String) {
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
    } else if link_references {
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

    let id = super::get_id_from_name(header);

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
