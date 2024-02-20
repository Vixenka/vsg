use chrono::{DateTime, Utc};
use flate2::read::GzDecoder;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tar::Archive;

#[derive(Serialize, Deserialize)]
struct Release {
    tag_name: String,
    published_at: DateTime<Utc>,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let client = Client::new();
    let release = get_latest_release(&client).await;

    let binaries = client
        .get(format!(
            "https://github.com/Vixenka/vsm/releases/download/{release}/Linux-amd64.tar.gz"
        ))
        .send()
        .await
        .expect("Unable to get binaries");
    let binaries = binaries
        .bytes()
        .await
        .expect("Unable to get bytes")
        .to_vec();
    let tar = GzDecoder::new(binaries.as_slice());

    let mut archive = Archive::new(tar);
    archive.unpack(".").expect("Unable to unpack");
    tracing::info!("Unpacked binaries");
}

async fn get_latest_release(client: &Client) -> String {
    let mut releases = client
        .get("https://api.github.com/repos/Vixenka/vsm/releases")
        .header("User-Agent", "vsm_updater")
        .send()
        .await
        .expect("Failed to get releases")
        .json::<Vec<Release>>()
        .await
        .expect("Failed to get releases string");

    releases.sort_by(|a, b| a.published_at.cmp(&b.published_at));
    let release = releases.pop().expect("No releases found");

    tracing::info!("Latest release: {}", &release.tag_name);
    release.tag_name
}
