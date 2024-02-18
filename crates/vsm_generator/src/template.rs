use std::{fs, path::Path};

use walkdir::DirEntry;

#[derive(Debug)]
pub struct Template {
    pub name: String,
    pub data: String,
}

impl Template {
    pub fn load(file: &DirEntry) -> anyhow::Result<Self> {
        Ok(Self {
            name: Path::new(file.file_name())
                .with_extension("")
                .to_str()
                .unwrap()
                .to_owned(),
            data: fs::read_to_string(file.path()).unwrap(),
        })
    }
}
