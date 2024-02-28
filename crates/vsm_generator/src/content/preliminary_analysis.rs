use std::{path::PathBuf, sync::Arc};

use crate::Context;

use super::{
    content_variables::ContentVariables,
    markdown::{self, MarkdownContent},
};

pub struct PreliminaryAnalysisOutput {
    pub path: PathBuf,
    pub template_path: PathBuf,
    pub variables: ContentVariables,
    pub content: Option<MarkdownContent>,
}

pub async fn analyze_file(
    context: Arc<Context>,
    path: PathBuf,
) -> anyhow::Result<PreliminaryAnalysisOutput> {
    tracing::trace!("Analyzing file '{}'", path.display());

    let mut variables = ContentVariables::new();
    variables.insert("link".to_owned(), context.get_file_link(&path));

    let mut content = None;
    let template_path = match path.extension().expect("Unable to get extension").to_str() {
        Some("md") => {
            let set_variable = markdown::set_variables(&context, &path, &mut variables);
            let template_path = markdown::get_template(&context, &path);

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

pub async fn create_md_post_list(
    outputs: &[Arc<PreliminaryAnalysisOutput>],
) -> anyhow::Result<String> {
    let mut result = String::new();

    let mut vec = outputs
        .iter()
        .filter_map(|v| match &v.content {
            Some(c) => Some(c),
            None => None,
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
