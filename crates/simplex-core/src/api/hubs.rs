use serde::{Deserialize, Serialize};
use thiserror::Error;
use super::library::MetadataItem;
use crate::cache::{self, CachePolicy};

const HUBS_TTL: CachePolicy = CachePolicy { ttl_secs: 60 };

#[derive(Debug, Error)]
pub enum HubError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HubsResponse {
    #[serde(rename = "MediaContainer")]
    pub media_container: HubsContainer,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HubsContainer {
    #[serde(default, rename = "Hub")]
    pub hubs: Vec<Hub>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hub {
    pub title: String,
    #[serde(rename = "type")]
    pub hub_type: Option<String>,
    #[serde(rename = "hubIdentifier")]
    pub hub_identifier: Option<String>,
    pub size: Option<u32>,
    #[serde(default, rename = "Metadata")]
    pub metadata: Vec<MetadataItem>,
}

/// Get hubs (continue watching, recently added, on deck, etc.)
pub async fn get_hubs(base_url: &str, token: &str) -> Result<Vec<Hub>, HubError> {
    let key = format!("hubs:get_hubs:{base_url}");
    if let Some(cached) = cache::get::<Vec<Hub>>(&key, HUBS_TTL) {
        return Ok(cached);
    }

    let client = super::plex_client(token)?;
    let url = format!("{}/hubs", base_url.trim_end_matches('/'));
    let resp: HubsResponse = client.get(&url).send().await?.json().await?;
    let hubs = resp.media_container.hubs;
    cache::set(&key, &hubs);
    Ok(hubs)
}

/// Get continue watching hub specifically.
pub async fn get_continue_watching(base_url: &str, token: &str) -> Result<Vec<MetadataItem>, HubError> {
    let hubs = get_hubs(base_url, token).await?;
    let items = hubs.into_iter()
        .filter(|h| {
            h.hub_identifier.as_deref() == Some("home.continue")
                || h.hub_identifier.as_deref() == Some("hub.tv.inprogress")
                || h.title.to_lowercase().contains("continue watching")
        })
        .flat_map(|h| h.metadata)
        .collect();
    Ok(items)
}

/// Get on deck items.
pub async fn get_on_deck(base_url: &str, token: &str) -> Result<Vec<MetadataItem>, HubError> {
    let hubs = get_hubs(base_url, token).await?;
    let items = hubs.into_iter()
        .filter(|h| {
            h.hub_identifier.as_deref() == Some("hub.tv.ondeck")
                || h.title.to_lowercase().contains("on deck")
        })
        .flat_map(|h| h.metadata)
        .collect();
    Ok(items)
}

/// Remove duplicate items across hub sections. Items are identified by
/// `rating_key`; an item only appears in the first hub that contains it.
/// Hubs that become empty after deduplication are dropped.
pub fn deduplicate_hubs(hubs: Vec<Hub>) -> Vec<Hub> {
    let mut seen = std::collections::HashSet::new();
    hubs.into_iter()
        .map(|mut hub| {
            hub.metadata.retain(|item| seen.insert(item.rating_key.clone()));
            hub
        })
        .filter(|hub| !hub.metadata.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_hub(title: &str, identifier: Option<&str>, items: Vec<&str>) -> Hub {
        Hub {
            title: title.to_string(),
            hub_type: None,
            hub_identifier: identifier.map(String::from),
            size: Some(items.len() as u32),
            metadata: items.into_iter().map(|t| MetadataItem {
                rating_key: "1".to_string(),
                key: "/library/metadata/1".to_string(),
                guid: None,
                external_guids: vec![],
                title: t.to_string(),
                media_type: None,
                summary: None,
                year: None,
                originally_available_at: None,
                thumb: None,
                art: None,
                parent_thumb: None,
                grandparent_thumb: None,
                duration: None,
                added_at: None,
                updated_at: None,
                view_count: None,
                rating: None,
                audience_rating: None,
                user_rating: None,
                album_type: None,
                parent_year: None,
                last_viewed_at: None,
                view_offset: None,
                parent_rating_key: None,
                grandparent_rating_key: None,
                parent_title: None,
                grandparent_title: None,
                parent_index: None,
                index: None,
                leaf_count: None,
                viewed_leaf_count: None,
                media: None,
            }).collect(),
        }
    }

    #[test]
    fn test_hubs_response_deserialize() {
        let json = r#"{
            "MediaContainer": {
                "Hub": [
                    {"title": "Continue Watching", "hubIdentifier": "home.continue", "size": 1, "Metadata": [{"ratingKey": "1", "key": "/library/metadata/1", "title": "Show A"}]},
                    {"title": "Recently Added", "hubIdentifier": "home.recent", "size": 0}
                ]
            }
        }"#;
        let resp: HubsResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.media_container.hubs.len(), 2);
        assert_eq!(resp.media_container.hubs[0].title, "Continue Watching");
        assert_eq!(resp.media_container.hubs[0].metadata.len(), 1);
        assert_eq!(resp.media_container.hubs[1].metadata.len(), 0);
    }

    #[test]
    fn test_hubs_empty() {
        let json = r#"{"MediaContainer": {"Hub": []}}"#;
        let resp: HubsResponse = serde_json::from_str(json).unwrap();
        assert!(resp.media_container.hubs.is_empty());
    }

    #[test]
    fn test_continue_watching_filter() {
        let hubs = vec![
            make_hub("Continue Watching", Some("home.continue"), vec!["Item A"]),
            make_hub("Recently Added", Some("home.recent"), vec!["Item B"]),
            make_hub("In Progress", Some("hub.tv.inprogress"), vec!["Item C"]),
        ];

        let cw: Vec<_> = hubs.into_iter()
            .filter(|h| {
                h.hub_identifier.as_deref() == Some("home.continue")
                    || h.hub_identifier.as_deref() == Some("hub.tv.inprogress")
                    || h.title.to_lowercase().contains("continue watching")
            })
            .flat_map(|h| h.metadata)
            .collect();

        assert_eq!(cw.len(), 2);
        assert_eq!(cw[0].title, "Item A");
        assert_eq!(cw[1].title, "Item C");
    }

    #[test]
    fn test_on_deck_filter() {
        let hubs = vec![
            make_hub("On Deck", Some("hub.tv.ondeck"), vec!["Episode 1"]),
            make_hub("Continue Watching", Some("home.continue"), vec!["Movie A"]),
        ];

        let od: Vec<_> = hubs.into_iter()
            .filter(|h| {
                h.hub_identifier.as_deref() == Some("hub.tv.ondeck")
                    || h.title.to_lowercase().contains("on deck")
            })
            .flat_map(|h| h.metadata)
            .collect();

        assert_eq!(od.len(), 1);
        assert_eq!(od[0].title, "Episode 1");
    }

    // -- deduplicate_hubs --

    fn make_hub_with_keys(title: &str, items: Vec<(&str, &str)>) -> Hub {
        Hub {
            title: title.to_string(),
            hub_type: None,
            hub_identifier: None,
            size: Some(items.len() as u32),
            metadata: items.into_iter().map(|(key, name)| MetadataItem {
                rating_key: key.to_string(),
                key: format!("/library/metadata/{}", key),
                guid: None,
                external_guids: vec![],
                title: name.to_string(),
                media_type: None,
                summary: None,
                year: None,
                originally_available_at: None,
                thumb: None,
                art: None,
                parent_thumb: None,
                grandparent_thumb: None,
                duration: None,
                added_at: None,
                updated_at: None,
                view_count: None,
                rating: None,
                audience_rating: None,
                user_rating: None,
                album_type: None,
                parent_year: None,
                last_viewed_at: None,
                view_offset: None,
                parent_rating_key: None,
                grandparent_rating_key: None,
                parent_title: None,
                grandparent_title: None,
                parent_index: None,
                index: None,
                leaf_count: None,
                viewed_leaf_count: None,
                media: None,
            }).collect(),
        }
    }

    #[test]
    fn test_deduplicate_removes_overlap() {
        let hubs = vec![
            make_hub_with_keys("Continue Watching", vec![("1", "Show A"), ("2", "Show B")]),
            make_hub_with_keys("On Deck", vec![("2", "Show B"), ("3", "Show C")]),
        ];
        let result = deduplicate_hubs(hubs);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].metadata.len(), 2);
        assert_eq!(result[1].metadata.len(), 1);
        assert_eq!(result[1].metadata[0].title, "Show C");
    }

    #[test]
    fn test_deduplicate_drops_empty_hubs() {
        let hubs = vec![
            make_hub_with_keys("Continue Watching", vec![("1", "Show A"), ("2", "Show B")]),
            make_hub_with_keys("On Deck", vec![("1", "Show A"), ("2", "Show B")]),
        ];
        let result = deduplicate_hubs(hubs);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].title, "Continue Watching");
    }

    #[test]
    fn test_deduplicate_preserves_order() {
        let hubs = vec![
            make_hub_with_keys("Hub 1", vec![("1", "A"), ("3", "C")]),
            make_hub_with_keys("Hub 2", vec![("2", "B"), ("3", "C"), ("4", "D")]),
            make_hub_with_keys("Hub 3", vec![("4", "D"), ("5", "E")]),
        ];
        let result = deduplicate_hubs(hubs);
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].metadata.len(), 2); // A, C
        assert_eq!(result[1].metadata.len(), 2); // B, D
        assert_eq!(result[2].metadata.len(), 1); // E
    }

    #[test]
    fn test_deduplicate_no_overlap() {
        let hubs = vec![
            make_hub_with_keys("Hub 1", vec![("1", "A")]),
            make_hub_with_keys("Hub 2", vec![("2", "B")]),
        ];
        let result = deduplicate_hubs(hubs);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].metadata.len(), 1);
        assert_eq!(result[1].metadata.len(), 1);
    }

    #[test]
    fn test_deduplicate_empty_input() {
        let result = deduplicate_hubs(vec![]);
        assert!(result.is_empty());
    }

    #[test]
    fn test_deduplicate_single_hub() {
        let hubs = vec![
            make_hub_with_keys("Hub 1", vec![("1", "A"), ("2", "B")]),
        ];
        let result = deduplicate_hubs(hubs);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].metadata.len(), 2);
    }
}
