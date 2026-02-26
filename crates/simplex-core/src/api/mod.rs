pub mod auth;
pub mod hubs;
pub mod library;
pub mod playback;
pub mod playlists;
pub mod search;
pub mod transcode;
pub mod users;

use reqwest::header::{HeaderMap, HeaderValue};
use std::time::Duration;

const DEFAULT_TIMEOUT_SECS: u64 = 10;

/// Build a reqwest client with standard Plex headers.
pub fn plex_client(token: &str) -> Result<reqwest::Client, reqwest::Error> {
    let mut headers = HeaderMap::new();
    headers.insert("X-Plex-Token", HeaderValue::from_str(token).unwrap_or_else(|_| HeaderValue::from_static("")));
    headers.insert("X-Plex-Product", HeaderValue::from_static("Simplex"));
    headers.insert("X-Plex-Version", HeaderValue::from_static("0.1.0"));
    headers.insert("X-Plex-Client-Identifier", HeaderValue::from_static("simplex-app"));
    headers.insert("Accept", HeaderValue::from_static("application/json"));

    reqwest::Client::builder()
        .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
        .default_headers(headers)
        .build()
}
