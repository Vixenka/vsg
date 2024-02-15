use std::{
    collections::HashMap,
    fs::{self, File},
    io::{Read, Write},
    path::PathBuf,
};

use crate::content::ContentCache;
use serde::{Deserialize, Serialize};

#[derive(Debug)]
pub struct Cache {
    path: PathBuf,
    inner: CacheInner,
}

impl Cache {
    #[tracing::instrument]
    pub fn load_or_new(path: PathBuf) -> anyhow::Result<Self> {
        let mut file = match File::open(&path) {
            Ok(file) => file,
            Err(error) => {
                if error.kind() == std::io::ErrorKind::NotFound {
                    return Ok(Self {
                        path,
                        inner: CacheInner::new(),
                    });
                }

                return Err(error.into());
            }
        };

        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer)?;

        let reader = flexbuffers::Reader::get_root(buffer.as_slice())?;
        let inner = match CacheInner::deserialize(reader) {
            Ok(inner) => {
                tracing::trace!("Loaded cache from file `{}`.", path.display());
                inner
            }
            Err(error) => {
                tracing::warn!("Unable to deserialize cache: {}.", error);
                CacheInner::new()
            }
        };

        Ok(Self { path, inner })
    }
}

impl Drop for Cache {
    #[tracing::instrument]
    fn drop(&mut self) {
        let mut serializer = flexbuffers::FlexbufferSerializer::new();
        self.inner
            .serialize(&mut serializer)
            .expect("Unable to serialize cache");

        fs::create_dir_all(
            self.path
                .parent()
                .expect("Unable to get parent of cache file"),
        )
        .expect("Unable to create cache directory");

        File::create(&self.path)
            .unwrap()
            .write_all(serializer.view())
            .expect("Unable to save cache file");

        tracing::trace!("Saved cache file.");
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct CacheInner {
    contents: HashMap<[u8; 32], ContentCache>,
}

impl CacheInner {
    #[tracing::instrument]
    pub fn new() -> Self {
        tracing::trace!("Creating default cache.");
        Self {
            contents: HashMap::new(),
        }
    }
}
