use serde::{Deserialize, Serialize};
use thiserror::Error;
use super::library::MetadataItem;

#[derive(Debug, Error)]
pub enum SearchError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResponse {
    #[serde(rename = "MediaContainer")]
    pub media_container: SearchContainer,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchContainer {
    #[serde(default, rename = "Metadata")]
    pub metadata: Vec<MetadataItem>,
}

/// Search for items on the server.
pub async fn search(base_url: &str, token: &str, query: &str) -> Result<Vec<MetadataItem>, SearchError> {
    let client = super::plex_client(token)?;
    let url = format!("{}/search", base_url.trim_end_matches('/'));
    let resp: SearchResponse = client.get(&url)
        .query(&[("query", query)])
        .send()
        .await?
        .json()
        .await?;
    Ok(resp.media_container.metadata)
}
