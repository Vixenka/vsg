use std::{fs, path::Path};

use walkdir::DirEntry;

#[derive(Debug)]
pub struct Template {
    pub name: String,
    pub data: Vec<u8>,
}

impl Template {
    pub fn load(file: &DirEntry) -> anyhow::Result<Self> {
        Ok(Self {
            name: Path::new(file.file_name())
                .with_extension("")
                .to_str()
                .unwrap()
                .to_owned(),
            data: fs::read(file.path()).unwrap(),
        })
    }
}
