use axum::{body::Body, extract::Request};
use r2d2::PooledConnection;
use r2d2_sqlite::{rusqlite::params, SqliteConnectionManager};
use std::{net::SocketAddr, sync::Arc};

use crate::AppState;

pub async fn prepare(connection: PooledConnection<SqliteConnectionManager>) {
    connection
        .execute(
            r#"
        CREATE TABLE IF NOT EXISTS analytics_raw (
            id INTEGER PRIMARY KEY,
            path TEXT,
            socket_addr TEXT,
            date DATETIME,
            headers TEXT,
            method TEXT
        )"#,
            params![],
        )
        .unwrap();
}

pub async fn push(
    state: Arc<AppState>,
    path: String,
    socket_addr: SocketAddr,
    request: Request<Body>,
) {
    if let Err(error) = state.database.pool.get().unwrap().execute(
        "INSERT INTO analytics_raw VALUES (null, ?, ?, DATETIME(), ?, ?)",
        params![
            path,
            socket_addr.to_string(),
            format!("{:?}", request.headers()),
            request.method().to_string()
        ],
    ) {
        tracing::error!("Failed to push analytics: {}", error);
    }
}
