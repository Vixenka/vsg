use crate::template::Template;
use std::{collections::HashMap, path::Path};
use walkdir::WalkDir;

#[derive(Debug)]
pub struct TemplateRepository {
    templates: HashMap<String, Template>,
}

impl TemplateRepository {
    #[tracing::instrument]
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let mut templates = HashMap::new();
        for file in WalkDir::new(path.join("templates"))
            .into_iter()
            .filter_map(|file| file.ok())
        {
            if !file.file_type().is_file() {
                continue;
            }

            let template = Template::load(&file)?;
            templates.insert(template.name.clone(), template);

            tracing::trace!("Loaded template: '{}'.", file.path().display());
        }

        Ok(Self { templates })
    }

    pub fn get(&self, name: &str) -> Option<&Template> {
        self.templates.get(name)
    }
}
