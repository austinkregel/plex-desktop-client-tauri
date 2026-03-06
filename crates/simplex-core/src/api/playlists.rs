use super::library::{MediaContainer, MetadataItem};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PlaylistError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Playlist {
    #[serde(rename = "ratingKey")]
    pub rating_key: String,
    pub key: String,
    pub title: String,
    #[serde(rename = "type")]
    pub playlist_type: Option<String>,
    pub summary: Option<String>,
    pub thumb: Option<String>,
    #[serde(rename = "leafCount")]
    pub leaf_count: Option<u32>,
    pub duration: Option<u64>,
    #[serde(rename = "addedAt")]
    pub added_at: Option<u64>,
    #[serde(rename = "updatedAt")]
    pub updated_at: Option<u64>,
}

/// Get all playlists.
pub async fn get_playlists(base_url: &str, token: &str) -> Result<Vec<Playlist>, PlaylistError> {
    let client = super::plex_client(token)?;
    let url = format!("{}/playlists", base_url.trim_end_matches('/'));
    let resp: MediaContainer<Playlist> = client.get(&url).send().await?.json().await?;
    Ok(resp.media_container.metadata)
}

/// Get items in a playlist.
pub async fn get_playlist_items(
    base_url: &str,
    token: &str,
    rating_key: &str,
) -> Result<Vec<MetadataItem>, PlaylistError> {
    let client = super::plex_client(token)?;
    let url = format!(
        "{}/playlists/{}/items",
        base_url.trim_end_matches('/'),
        rating_key
    );
    let resp: MediaContainer<MetadataItem> = client.get(&url).send().await?.json().await?;
    Ok(resp.media_container.metadata)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_playlist_deserialize() {
        let json = r#"{
            "MediaContainer": {
                "Metadata": [
                    {
                        "ratingKey": "100",
                        "key": "/playlists/100/items",
                        "title": "My Playlist",
                        "type": "playlist",
                        "leafCount": 25,
                        "duration": 54000
                    }
                ]
            }
        }"#;
        let container: MediaContainer<Playlist> = serde_json::from_str(json).unwrap();
        let playlists = &container.media_container.metadata;
        assert_eq!(playlists.len(), 1);
        assert_eq!(playlists[0].title, "My Playlist");
        assert_eq!(playlists[0].leaf_count, Some(25));
    }
}
