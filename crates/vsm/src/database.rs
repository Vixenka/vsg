use std::path::Path;

use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use tokio::fs;

use crate::{analytics, Args};

pub struct Database {
    pub pool: Pool<SqliteConnectionManager>,
}

impl Database {
    pub async fn open(args: &Args) -> anyhow::Result<Self> {
        fs::create_dir_all(&args.output).await?;

        let manager =
            SqliteConnectionManager::file(Path::new(&args.output).join("database.sqlite3"));
        let pool = r2d2::Pool::new(manager)?;

        analytics::prepare(pool.get()?).await;

        Ok(Self { pool })
    }
}
