use flate2::read::GzDecoder;
use reqwest::Client;
use tar::Archive;

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
}

async fn get_latest_release(client: &Client) -> String {
    let releases = client
        .get("https://api.github.com/repos/Vixenka/vsm/releases")
        .header("User-Agent", "vsm_updater")
        .send()
        .await
        .expect("Failed to get releases")
        .text()
        .await
        .expect("Failed to get releases string");

    const START_TEXT: &str = "\"tag_name\":\"";
    let start = releases.find(START_TEXT).expect("Failed to find start") + START_TEXT.len();
    let end = releases[start..].find('"').expect("Unable to find end") + start;

    let string = releases[start..end].to_owned();
    tracing::info!("Latest release: {}", &string);
    string
}
