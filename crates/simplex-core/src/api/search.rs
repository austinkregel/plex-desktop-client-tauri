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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_search_response_deserialize() {
        let json = r#"{
            "MediaContainer": {
                "Metadata": [
                    {
                        "ratingKey": "1",
                        "key": "/library/metadata/1",
                        "title": "Breaking Bad",
                        "type": "show"
                    }
                ]
            }
        }"#;
        let resp: SearchResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.media_container.metadata.len(), 1);
        assert_eq!(resp.media_container.metadata[0].title, "Breaking Bad");
    }

    #[test]
    fn test_search_response_empty_metadata() {
        let json = r#"{ "MediaContainer": {} }"#;
        let resp: SearchResponse = serde_json::from_str(json).unwrap();
        assert!(resp.media_container.metadata.is_empty());
    }

    #[test]
    fn test_search_container_default_metadata() {
        let json = r#"{ "MediaContainer": { "Metadata": [] } }"#;
        let resp: SearchResponse = serde_json::from_str(json).unwrap();
        assert!(resp.media_container.metadata.is_empty());
    }

    #[test]
    fn test_search_response_multiple_results() {
        let json = r#"{
            "MediaContainer": {
                "Metadata": [
                    { "ratingKey": "1", "key": "/library/metadata/1", "title": "Foo", "type": "movie" },
                    { "ratingKey": "2", "key": "/library/metadata/2", "title": "Bar", "type": "show" }
                ]
            }
        }"#;
        let resp: SearchResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.media_container.metadata.len(), 2);
        assert_eq!(resp.media_container.metadata[0].rating_key, "1");
        assert_eq!(resp.media_container.metadata[1].rating_key, "2");
    }

    #[test]
    fn test_search_response_roundtrip() {
        let resp = SearchResponse {
            media_container: SearchContainer {
                metadata: vec![],
            },
        };
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: SearchResponse = serde_json::from_str(&json).unwrap();
        assert!(parsed.media_container.metadata.is_empty());
    }
}
