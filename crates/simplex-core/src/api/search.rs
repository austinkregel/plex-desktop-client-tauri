use serde::{Deserialize, Serialize};
use thiserror::Error;
use super::library::MetadataItem;
use crate::cache::{self, CachePolicy};

const SEARCH_TTL: CachePolicy = CachePolicy { ttl_secs: 45 };

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
    let key = format!("search:query:{base_url}:{}", query.trim().to_lowercase());
    if let Some(cached) = cache::get::<Vec<MetadataItem>>(&key, SEARCH_TTL) {
        return Ok(cached);
    }

    let client = super::plex_client(token)?;
    let url = format!("{}/search", base_url.trim_end_matches('/'));
    let resp: SearchResponse = client.get(&url)
        .query(&[("query", query)])
        .send()
        .await?
        .json()
        .await?;
    let items = resp.media_container.metadata;
    cache::set(&key, &items);
    Ok(items)
}
