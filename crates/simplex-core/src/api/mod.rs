pub mod auth;
pub mod hubs;
pub mod library;
pub mod playback;
pub mod playlists;
pub mod search;
pub mod transcode;
pub mod users;

use reqwest::header::{HeaderMap, HeaderValue};
use serde::de::DeserializeOwned;
use std::time::Duration;

const DEFAULT_TIMEOUT_SECS: u64 = 10;

/// Deserialize a reqwest `Response` as JSON, logging the raw body and the
/// specific serde error when deserialization fails so transient API issues
/// are diagnosable instead of producing a generic "error decoding response body".
pub async fn json_response<T: DeserializeOwned>(resp: reqwest::Response) -> Result<T, String> {
    let url = resp.url().to_string();
    let status = resp.status();

    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Err(format!(
            "Unauthorized (401) from {url} — token may be expired"
        ));
    }

    if !status.is_success() {
        let preview: String = resp
            .text()
            .await
            .unwrap_or_default()
            .chars()
            .take(256)
            .collect();
        return Err(format!(
            "HTTP {status} from {url} — body preview: {preview}"
        ));
    }

    let body = resp
        .text()
        .await
        .map_err(|e| format!("Failed to read response body from {url}: {e}"))?;

    serde_json::from_str::<T>(&body).map_err(|serde_err| {
        let preview: String = body.chars().take(512).collect();
        tracing::warn!(
            "JSON decode failed for {} (HTTP {}): {} — body preview: {}",
            url,
            status,
            serde_err,
            preview,
        );
        format!("JSON decode error from {url}: {serde_err}")
    })
}

/// Build a reqwest client with standard Plex headers.
pub fn plex_client(token: &str) -> Result<reqwest::Client, reqwest::Error> {
    let mut headers = HeaderMap::new();
    headers.insert(
        "X-Plex-Token",
        HeaderValue::from_str(token).unwrap_or_else(|_| HeaderValue::from_static("")),
    );
    headers.insert("X-Plex-Product", HeaderValue::from_static("Simplex"));
    headers.insert("X-Plex-Version", HeaderValue::from_static("0.1.0"));
    headers.insert(
        "X-Plex-Client-Identifier",
        HeaderValue::from_static("simplex-app"),
    );
    headers.insert("Accept", HeaderValue::from_static("application/json"));

    reqwest::Client::builder()
        .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
        .default_headers(headers)
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plex_client_builds_successfully() {
        let client = plex_client("test-token-123");
        assert!(client.is_ok());
    }

    #[test]
    fn test_plex_client_with_empty_token() {
        let client = plex_client("");
        assert!(client.is_ok());
    }

    #[test]
    fn test_plex_client_with_special_characters() {
        let client = plex_client("tok\x00en");
        assert!(client.is_ok());
    }
}
