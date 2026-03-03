use serde::{Deserialize, Serialize};
use thiserror::Error;
use crate::cache::{self, CachePolicy};

const SECTIONS_TTL: CachePolicy = CachePolicy { ttl_secs: 300 };
const SECTION_ITEMS_TTL: CachePolicy = CachePolicy { ttl_secs: 90 };
const METADATA_TTL: CachePolicy = CachePolicy { ttl_secs: 120 };
const CHILDREN_TTL: CachePolicy = CachePolicy { ttl_secs: 120 };
const FILTER_OPTIONS_TTL: CachePolicy = CachePolicy { ttl_secs: 600 };

#[derive(Debug, Error)]
pub enum LibraryError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("Parse error: {0}")]
    Parse(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaContainer<T> {
    #[serde(rename = "MediaContainer")]
    pub media_container: MediaContainerInner<T>,
}

fn default_vec<T>() -> Vec<T> {
    Vec::new()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaContainerInner<T> {
    pub size: Option<u32>,
    #[serde(rename = "totalSize")]
    pub total_size: Option<u32>,
    #[serde(default = "default_vec", rename = "Metadata")]
    pub metadata: Vec<T>,
    #[serde(default = "default_vec", rename = "Directory")]
    pub directory: Vec<T>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibrarySection {
    pub key: String,
    pub title: String,
    #[serde(rename = "type")]
    pub section_type: String,
    pub agent: Option<String>,
    pub scanner: Option<String>,
    pub thumb: Option<String>,
    pub art: Option<String>,
    #[serde(rename = "updatedAt")]
    pub updated_at: Option<u64>,
}

fn de_string_or_number<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct StringOrNumberVisitor;

    impl<'de> serde::de::Visitor<'de> for StringOrNumberVisitor {
        type Value = String;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a string or number")
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(value.to_string())
        }

        fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(value)
        }

        fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(value.to_string())
        }

        fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(value.to_string())
        }
    }

    deserializer.deserialize_any(StringOrNumberVisitor)
}

/// Flexible deserializer that accepts a string, number, or null, defaulting to
/// an empty string. Used for fields like `ratingKey` and `key` that are required
/// in well-formed items but may be absent or numeric in virtual directory entries.
fn de_flexible_string<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct FlexVisitor;

    impl<'de> serde::de::Visitor<'de> for FlexVisitor {
        type Value = String;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a string, number, or null")
        }

        fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
            Ok(v.to_string())
        }

        fn visit_string<E: serde::de::Error>(self, v: String) -> Result<Self::Value, E> {
            Ok(v)
        }

        fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Self::Value, E> {
            Ok(v.to_string())
        }

        fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Self::Value, E> {
            Ok(v.to_string())
        }

        fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
            Ok(String::new())
        }

        fn visit_none<E: serde::de::Error>(self) -> Result<Self::Value, E> {
            Ok(String::new())
        }
    }

    deserializer.deserialize_any(FlexVisitor)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FilterOption {
    #[serde(deserialize_with = "de_string_or_number")]
    pub key: String,
    pub title: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryFilter {
    pub genre: Option<String>,
    pub year: Option<String>,
    pub content_rating: Option<String>,
    pub resolution: Option<String>,
    pub unwatched_only: bool,
    pub audio_language: Option<String>,
    pub sort: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuidEntry {
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Marker {
    pub id: Option<u64>,
    #[serde(rename = "type")]
    pub marker_type: Option<String>,
    #[serde(rename = "startTimeOffset")]
    pub start_time_offset: Option<u64>,
    #[serde(rename = "endTimeOffset")]
    pub end_time_offset: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetadataItem {
    #[serde(default, rename = "ratingKey", deserialize_with = "de_flexible_string")]
    pub rating_key: String,
    #[serde(default, deserialize_with = "de_flexible_string")]
    pub key: String,
    /// Plex agent GUID (e.g. "plex://movie/5d776…" or legacy "com.plexapp.agents.imdb://tt0111161").
    pub guid: Option<String>,
    /// External IDs from matched agents (IMDB, TMDB, TVDB).
    #[serde(default, rename = "Guid")]
    pub external_guids: Vec<GuidEntry>,
    #[serde(default)]
    pub title: String,
    #[serde(rename = "type")]
    pub media_type: Option<String>,
    pub summary: Option<String>,
    pub year: Option<u32>,
    #[serde(rename = "originallyAvailableAt")]
    pub originally_available_at: Option<String>,
    pub thumb: Option<String>,
    pub art: Option<String>,
    #[serde(rename = "parentThumb")]
    pub parent_thumb: Option<String>,
    #[serde(rename = "grandparentThumb")]
    pub grandparent_thumb: Option<String>,
    pub duration: Option<u64>,
    #[serde(rename = "addedAt")]
    pub added_at: Option<u64>,
    #[serde(rename = "updatedAt")]
    pub updated_at: Option<u64>,
    #[serde(rename = "viewCount")]
    pub view_count: Option<u32>,
    #[serde(rename = "rating")]
    pub rating: Option<f32>,
    #[serde(rename = "audienceRating")]
    pub audience_rating: Option<f32>,
    #[serde(rename = "userRating")]
    pub user_rating: Option<f32>,
    #[serde(rename = "albumType")]
    pub album_type: Option<String>,
    #[serde(rename = "parentYear")]
    pub parent_year: Option<u32>,
    #[serde(rename = "lastViewedAt")]
    pub last_viewed_at: Option<u64>,
    #[serde(rename = "viewOffset")]
    pub view_offset: Option<u64>,
    #[serde(rename = "parentRatingKey")]
    pub parent_rating_key: Option<String>,
    #[serde(rename = "grandparentRatingKey")]
    pub grandparent_rating_key: Option<String>,
    #[serde(rename = "parentTitle")]
    pub parent_title: Option<String>,
    #[serde(rename = "grandparentTitle")]
    pub grandparent_title: Option<String>,
    #[serde(rename = "parentIndex")]
    pub parent_index: Option<u32>,
    pub index: Option<u32>,
    #[serde(rename = "leafCount")]
    pub leaf_count: Option<u32>,
    #[serde(rename = "viewedLeafCount")]
    pub viewed_leaf_count: Option<u32>,
    #[serde(rename = "Media")]
    pub media: Option<Vec<MediaInfo>>,
    #[serde(default, rename = "Marker")]
    pub markers: Vec<Marker>,
}

impl MetadataItem {
    /// Returns the best available thumbnail path, falling back through
    /// thumb -> parentThumb -> grandparentThumb.
    pub fn best_thumb(&self) -> Option<&str> {
        self.thumb.as_deref()
            .or(self.parent_thumb.as_deref())
            .or(self.grandparent_thumb.as_deref())
    }

    /// Builds a full thumbnail URL using the best available thumb path.
    pub fn best_thumb_url(&self, base_url: &str, token: &str) -> Option<String> {
        self.best_thumb().map(|t| thumb_url(base_url, token, t))
    }

    /// Builds a human-readable subtitle for display.
    /// - Episodes: "Show Name S01E05"
    /// - Seasons/children: parent title
    /// - Movies/other: year
    pub fn display_subtitle(&self) -> Option<String> {
        if let Some(grandparent) = &self.grandparent_title {
            let mut sub = grandparent.clone();
            if let (Some(si), Some(ei)) = (self.parent_index, self.index) {
                sub.push_str(&format!(" S{:02}E{:02}", si, ei));
            }
            return Some(sub);
        }
        if let Some(parent) = &self.parent_title {
            return Some(parent.clone());
        }
        self.year.map(|y| y.to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaInfo {
    pub id: Option<u64>,
    pub duration: Option<u64>,
    pub bitrate: Option<u32>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    #[serde(rename = "videoCodec")]
    pub video_codec: Option<String>,
    #[serde(rename = "audioCodec")]
    pub audio_codec: Option<String>,
    #[serde(rename = "audioChannels")]
    pub audio_channels: Option<u32>,
    pub container: Option<String>,
    #[serde(rename = "videoResolution")]
    pub video_resolution: Option<String>,
    #[serde(rename = "Part")]
    pub parts: Option<Vec<MediaPart>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaPart {
    pub id: Option<u64>,
    pub key: Option<String>,
    pub duration: Option<u64>,
    pub file: Option<String>,
    pub size: Option<u64>,
    pub container: Option<String>,
    #[serde(rename = "Stream")]
    pub streams: Option<Vec<MediaStream>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaStream {
    pub id: Option<u64>,
    #[serde(rename = "streamType")]
    pub stream_type: Option<u32>,
    pub codec: Option<String>,
    pub language: Option<String>,
    #[serde(rename = "languageCode")]
    pub language_code: Option<String>,
    #[serde(rename = "displayTitle")]
    pub display_title: Option<String>,
    pub channels: Option<u32>,
    pub selected: Option<bool>,
    #[serde(rename = "forced")]
    pub forced: Option<bool>,
    pub index: Option<u32>,
}

/// Get all library sections (Movies, TV Shows, Music, etc.)
pub async fn get_sections(base_url: &str, token: &str) -> Result<Vec<LibrarySection>, LibraryError> {
    let key = format!("library:get_sections:{base_url}");
    if let Some(cached) = cache::get::<Vec<LibrarySection>>(&key, SECTIONS_TTL) {
        return Ok(cached);
    }

    let client = super::plex_client(token)?;
    let url = format!("{}/library/sections", base_url.trim_end_matches('/'));
    let resp: MediaContainer<LibrarySection> = client.get(&url).send().await?.json().await?;
    let sections = resp.media_container.directory;
    cache::set(&key, &sections);
    Ok(sections)
}

/// Get all items in a library section.
pub async fn get_section_items(base_url: &str, token: &str, section_key: &str) -> Result<Vec<MetadataItem>, LibraryError> {
    get_section_items_filtered(base_url, token, section_key, &LibraryFilter::default()).await
}

fn build_library_filter_query(filter: &LibraryFilter) -> Vec<(String, String)> {
    let mut query = Vec::new();
    if let Some(genre) = &filter.genre {
        query.push(("genre".to_string(), genre.clone()));
    }
    if let Some(year) = &filter.year {
        query.push(("year".to_string(), year.clone()));
    }
    if let Some(content_rating) = &filter.content_rating {
        query.push(("contentRating".to_string(), content_rating.clone()));
    }
    if let Some(resolution) = &filter.resolution {
        query.push(("resolution".to_string(), resolution.clone()));
    }
    if filter.unwatched_only {
        query.push(("unwatched".to_string(), "1".to_string()));
    }
    if let Some(audio_language) = &filter.audio_language {
        query.push(("audioLanguage".to_string(), audio_language.clone()));
    }
    if let Some(sort) = &filter.sort {
        query.push(("sort".to_string(), sort.clone()));
    }
    query
}

/// Get all items in a library section, with optional sort and filter query params.
pub async fn get_section_items_filtered(
    base_url: &str,
    token: &str,
    section_key: &str,
    filter: &LibraryFilter,
) -> Result<Vec<MetadataItem>, LibraryError> {
    let key = format!("library:get_section_items_filtered:{base_url}:{section_key}:{}", serde_json::to_string(filter).unwrap_or_default());
    if let Some(cached) = cache::get::<Vec<MetadataItem>>(&key, SECTION_ITEMS_TTL) {
        return Ok(cached);
    }

    let client = super::plex_client(token)?;
    let url = format!("{}/library/sections/{}/all", base_url.trim_end_matches('/'), section_key);
    let query = build_library_filter_query(filter);
    let mut req = client.get(&url);
    if !query.is_empty() {
        req = req.query(&query);
    }
    let resp: MediaContainer<MetadataItem> = req.send().await?.json().await?;
    let items = resp.media_container.metadata;
    cache::set(&key, &items);
    Ok(items)
}

/// Fetch filter values for a section (e.g. genre, year, contentRating, resolution, language).
pub async fn get_filter_options(
    base_url: &str,
    token: &str,
    section_key: &str,
    filter_type: &str,
) -> Result<Vec<FilterOption>, LibraryError> {
    let key = format!("library:get_filter_options:{base_url}:{section_key}:{filter_type}");
    if let Some(cached) = cache::get::<Vec<FilterOption>>(&key, FILTER_OPTIONS_TTL) {
        return Ok(cached);
    }

    let client = super::plex_client(token)?;
    let url = format!(
        "{}/library/sections/{}/{}",
        base_url.trim_end_matches('/'),
        section_key,
        filter_type
    );
    let resp: MediaContainer<FilterOption> = client.get(&url).send().await?.json().await?;
    let options = resp.media_container.directory;
    cache::set(&key, &options);
    Ok(options)
}

/// Get metadata for a specific item.
pub async fn get_metadata(base_url: &str, token: &str, rating_key: &str) -> Result<MetadataItem, LibraryError> {
    let key = format!("library:get_metadata:{base_url}:{rating_key}");
    if let Some(cached) = cache::get::<MetadataItem>(&key, METADATA_TTL) {
        return Ok(cached);
    }

    let client = super::plex_client(token)?;
    let url = format!(
        "{}/library/metadata/{}?includeMarkers=1",
        base_url.trim_end_matches('/'),
        rating_key
    );
    let resp: MediaContainer<MetadataItem> = client.get(&url).send().await?.json().await?;
    let item = resp.media_container.metadata.into_iter().next()
        .ok_or_else(|| LibraryError::Parse("No metadata found".to_string()))?;
    cache::set(&key, &item);
    Ok(item)
}

/// Get children of an item (seasons for a show, episodes for a season).
/// Merges both `Metadata` and `Directory` arrays from the response since Plex
/// returns container children (seasons, albums) in `Directory` and leaf
/// children (episodes, tracks) in `Metadata`.
pub async fn get_children(base_url: &str, token: &str, rating_key: &str) -> Result<Vec<MetadataItem>, LibraryError> {
    let key = format!("library:get_children:{base_url}:{rating_key}");
    if let Some(cached) = cache::get::<Vec<MetadataItem>>(&key, CHILDREN_TTL) {
        return Ok(cached);
    }

    let client = super::plex_client(token)?;
    let url = format!("{}/library/metadata/{}/children", base_url.trim_end_matches('/'), rating_key);
    let resp: MediaContainer<MetadataItem> = client.get(&url).send().await?.json().await?;
    let mut items = resp.media_container.metadata;
    items.extend(resp.media_container.directory);
    items.retain(|i| !i.rating_key.is_empty());
    cache::set(&key, &items);
    Ok(items)
}

/// Get collections in a library section.
pub async fn get_collections(base_url: &str, token: &str, section_key: &str) -> Result<Vec<MetadataItem>, LibraryError> {
    let client = super::plex_client(token)?;
    let url = format!("{}/library/sections/{}/collections", base_url.trim_end_matches('/'), section_key);
    let resp: MediaContainer<MetadataItem> = client.get(&url).send().await?.json().await?;
    Ok(resp.media_container.metadata)
}

/// Adjacent episode result.
pub struct AdjacentEpisodes {
    pub previous: Option<MetadataItem>,
    pub current: MetadataItem,
    pub next: Option<MetadataItem>,
}

/// Pure helper: given a list of siblings and a target rating_key, find the
/// previous, current, and next items. Returns `None` if the key is not found.
pub fn find_adjacent(siblings: &[MetadataItem], rating_key: &str) -> Option<AdjacentEpisodes> {
    let pos = siblings.iter().position(|i| i.rating_key == rating_key)?;
    let previous = if pos > 0 { Some(siblings[pos - 1].clone()) } else { None };
    let next = siblings.get(pos + 1).cloned();
    Some(AdjacentEpisodes {
        previous,
        current: siblings[pos].clone(),
        next,
    })
}

/// Fetch the current item's metadata, then its siblings, and return
/// previous/current/next for episode navigation.
///
/// For episodes, uses `parentRatingKey` (the season) to fetch sibling episodes.
/// Falls back to the item itself if no parent key is available (e.g. movies).
pub async fn get_adjacent_episodes(
    base_url: &str,
    token: &str,
    rating_key: &str,
) -> Result<AdjacentEpisodes, LibraryError> {
    let item = get_metadata(base_url, token, rating_key).await?;

    let parent_rk = item.parent_rating_key.as_deref()
        .ok_or_else(|| LibraryError::Parse(
            format!("Item {} has no parentRatingKey -- cannot find siblings", rating_key)
        ))?;

    let siblings = get_children(base_url, token, parent_rk).await?;
    find_adjacent(&siblings, rating_key)
        .ok_or_else(|| LibraryError::Parse(format!("Item {} not found in siblings", rating_key)))
}

/// Build the thumbnail URL for an item.
pub fn thumb_url(base_url: &str, token: &str, thumb_path: &str) -> String {
    format!("{}{}?X-Plex-Token={}", base_url.trim_end_matches('/'), thumb_path, token)
}

#[derive(Debug, Clone)]
pub struct ArtistDiscography {
    pub popular_tracks: Vec<MetadataItem>,
    pub albums: Vec<MetadataItem>,
    pub eps_and_singles: Vec<MetadataItem>,
}

fn sort_popular_tracks(mut tracks: Vec<MetadataItem>) -> Vec<MetadataItem> {
    tracks.sort_by(|a, b| {
        let a_score = a.user_rating.or(a.audience_rating).or(a.rating).unwrap_or(0.0);
        let b_score = b.user_rating.or(b.audience_rating).or(b.rating).unwrap_or(0.0);
        b_score
            .partial_cmp(&a_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.view_count.unwrap_or(0).cmp(&a.view_count.unwrap_or(0)))
    });
    tracks
}

fn split_discography_items(mut items: Vec<MetadataItem>) -> (Vec<MetadataItem>, Vec<MetadataItem>) {
    items.retain(|i| i.media_type.as_deref() == Some("album"));
    let mut albums = Vec::new();
    let mut eps_and_singles = Vec::new();
    for album in items {
        let album_kind = album.album_type.clone().unwrap_or_default().to_lowercase();
        if album_kind.contains("ep") || album_kind.contains("single") {
            eps_and_singles.push(album);
        } else {
            albums.push(album);
        }
    }

    albums.sort_by(|a, b| {
        a.year
            .or(a.parent_year)
            .unwrap_or(0)
            .cmp(&b.year.or(b.parent_year).unwrap_or(0))
            .then_with(|| a.title.cmp(&b.title))
    });
    eps_and_singles.sort_by(|a, b| {
        a.year
            .or(a.parent_year)
            .unwrap_or(0)
            .cmp(&b.year.or(b.parent_year).unwrap_or(0))
            .then_with(|| a.title.cmp(&b.title))
    });

    (albums, eps_and_singles)
}

pub async fn get_artist_discography(
    base_url: &str,
    token: &str,
    artist_rating_key: &str,
) -> Result<ArtistDiscography, LibraryError> {
    let artist_children = get_children(base_url, token, artist_rating_key).await?;
    let mut all_tracks = Vec::new();
    for album in &artist_children {
        if album.media_type.as_deref() == Some("album") {
            let mut tracks = get_children(base_url, token, &album.rating_key).await.unwrap_or_default();
            all_tracks.append(&mut tracks);
        }
    }

    let mut popular_tracks = sort_popular_tracks(all_tracks.clone());
    popular_tracks.truncate(10);
    let (albums, eps_and_singles) = split_discography_items(artist_children);

    Ok(ArtistDiscography {
        popular_tracks,
        albums,
        eps_and_singles,
    })
}

/// Find the first unwatched or partially-watched episode for a show.
/// Iterates seasons in order, then episodes within each season.
/// Returns the first episode with `view_offset > 0` (resume), or
/// the first episode with `view_count` absent/0 (unwatched).
pub async fn get_next_episode(
    base_url: &str,
    token: &str,
    show_rating_key: &str,
) -> Result<Option<MetadataItem>, LibraryError> {
    let mut seasons = get_children(base_url, token, show_rating_key).await?;
    seasons.retain(|s| s.media_type.as_deref() == Some("season"));
    seasons.sort_by_key(|s| s.index.unwrap_or(u32::MAX));

    for season in &seasons {
        let mut episodes = get_children(base_url, token, &season.rating_key).await.unwrap_or_default();
        episodes.sort_by_key(|e| e.index.unwrap_or(u32::MAX));

        // Prefer a partially-watched episode first (resume point).
        if let Some(ep) = episodes.iter().find(|e| e.view_offset.unwrap_or(0) > 0) {
            return Ok(Some(ep.clone()));
        }

        // Otherwise find the first completely unwatched episode.
        if let Some(ep) = episodes.iter().find(|e| e.view_count.unwrap_or(0) == 0) {
            return Ok(Some(ep.clone()));
        }
    }

    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_library_sections_deserialize() {
        let json = r#"{
            "MediaContainer": {
                "size": 3,
                "Directory": [
                    {"key": "1", "title": "Movies", "type": "movie", "agent": "tv.plex.agents.movie", "scanner": "Plex Movie"},
                    {"key": "2", "title": "TV Shows", "type": "show"},
                    {"key": "3", "title": "Music", "type": "artist"}
                ]
            }
        }"#;
        let container: MediaContainer<LibrarySection> = serde_json::from_str(json).unwrap();
        assert_eq!(container.media_container.directory.len(), 3);
        assert_eq!(container.media_container.directory[0].key, "1");
        assert_eq!(container.media_container.directory[0].title, "Movies");
        assert_eq!(container.media_container.directory[0].section_type, "movie");
    }

    #[test]
    fn test_metadata_item_deserialize() {
        let json = r#"{
            "MediaContainer": {
                "size": 1,
                "Metadata": [
                    {
                        "ratingKey": "12345",
                        "key": "/library/metadata/12345",
                        "title": "The Matrix",
                        "type": "movie",
                        "year": 1999,
                        "summary": "A computer hacker learns about the true nature of reality.",
                        "duration": 8160000,
                        "viewCount": 3,
                        "viewOffset": 4500000,
                        "thumb": "/library/metadata/12345/thumb/1234567890",
                        "art": "/library/metadata/12345/art/1234567890"
                    }
                ]
            }
        }"#;
        let container: MediaContainer<MetadataItem> = serde_json::from_str(json).unwrap();
        let items = &container.media_container.metadata;
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].rating_key, "12345");
        assert_eq!(items[0].title, "The Matrix");
        assert_eq!(items[0].year, Some(1999));
        assert_eq!(items[0].view_count, Some(3));
        assert_eq!(items[0].view_offset, Some(4500000));
        assert!(items[0].guid.is_none());
        assert!(items[0].external_guids.is_empty());
    }

    #[test]
    fn test_metadata_item_with_guids() {
        let json = r#"{
            "MediaContainer": {
                "Metadata": [{
                    "ratingKey": "99",
                    "key": "/library/metadata/99",
                    "title": "The Matrix",
                    "guid": "plex://movie/5d776824880197001ec967c1",
                    "Guid": [
                        {"id": "imdb://tt0133093"},
                        {"id": "tmdb://603"},
                        {"id": "tvdb://111"}
                    ]
                }]
            }
        }"#;
        let container: MediaContainer<MetadataItem> = serde_json::from_str(json).unwrap();
        let item = &container.media_container.metadata[0];
        assert_eq!(item.guid.as_deref(), Some("plex://movie/5d776824880197001ec967c1"));
        assert_eq!(item.external_guids.len(), 3);
        assert_eq!(item.external_guids[0].id, "imdb://tt0133093");
        assert_eq!(item.external_guids[1].id, "tmdb://603");
        assert_eq!(item.external_guids[2].id, "tvdb://111");
    }

    #[test]
    fn test_metadata_with_media_streams() {
        let json = r#"{
            "MediaContainer": {
                "Metadata": [{
                    "ratingKey": "1",
                    "key": "/library/metadata/1",
                    "title": "Test",
                    "Media": [{
                        "id": 100,
                        "duration": 5400000,
                        "videoCodec": "h264",
                        "audioCodec": "aac",
                        "audioChannels": 6,
                        "videoResolution": "1080",
                        "Part": [{
                            "id": 200,
                            "key": "/library/parts/200/file.mkv",
                            "file": "/data/movies/test.mkv",
                            "Stream": [
                                {"id": 1, "streamType": 1, "codec": "h264", "displayTitle": "1080p (H.264)"},
                                {"id": 2, "streamType": 2, "codec": "aac", "language": "English", "languageCode": "eng", "channels": 6, "selected": true},
                                {"id": 3, "streamType": 3, "codec": "srt", "language": "English", "languageCode": "eng", "forced": false}
                            ]
                        }]
                    }]
                }]
            }
        }"#;
        let container: MediaContainer<MetadataItem> = serde_json::from_str(json).unwrap();
        let item = &container.media_container.metadata[0];
        let media = item.media.as_ref().unwrap();
        assert_eq!(media.len(), 1);
        assert_eq!(media[0].video_codec, Some("h264".to_string()));
        assert_eq!(media[0].audio_channels, Some(6));
        let part = &media[0].parts.as_ref().unwrap()[0];
        let streams = part.streams.as_ref().unwrap();
        assert_eq!(streams.len(), 3);
        assert_eq!(streams[1].language, Some("English".to_string()));
        assert_eq!(streams[1].selected, Some(true));
        assert_eq!(streams[0].stream_type, Some(1));
        assert_eq!(streams[1].stream_type, Some(2));
        assert_eq!(streams[2].stream_type, Some(3));
    }

    #[test]
    fn test_metadata_empty_container() {
        let json = r#"{"MediaContainer": {"size": 0}}"#;
        let container: MediaContainer<MetadataItem> = serde_json::from_str(json).unwrap();
        assert!(container.media_container.metadata.is_empty());
        assert!(container.media_container.directory.is_empty());
    }

    #[test]
    fn test_thumb_url() {
        let url = thumb_url("http://localhost:32400", "mytoken", "/library/metadata/123/thumb/456");
        assert_eq!(url, "http://localhost:32400/library/metadata/123/thumb/456?X-Plex-Token=mytoken");
    }

    #[test]
    fn test_thumb_url_trailing_slash() {
        let url = thumb_url("http://localhost:32400/", "tok", "/thumb/1");
        assert_eq!(url, "http://localhost:32400/thumb/1?X-Plex-Token=tok");
    }

    fn make_item(overrides: impl FnOnce(&mut MetadataItem)) -> MetadataItem {
        let mut item = MetadataItem {
            rating_key: "1".into(),
            key: "/library/metadata/1".into(),
            guid: None,
            external_guids: vec![],
            title: "Test".into(),
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
            markers: vec![],
        };
        overrides(&mut item);
        item
    }

    // -- best_thumb tests --

    #[test]
    fn test_best_thumb_prefers_own_thumb() {
        let item = make_item(|i| {
            i.thumb = Some("/thumb/own".into());
            i.parent_thumb = Some("/thumb/parent".into());
            i.grandparent_thumb = Some("/thumb/gp".into());
        });
        assert_eq!(item.best_thumb(), Some("/thumb/own"));
    }

    #[test]
    fn test_best_thumb_falls_back_to_parent() {
        let item = make_item(|i| {
            i.parent_thumb = Some("/thumb/parent".into());
            i.grandparent_thumb = Some("/thumb/gp".into());
        });
        assert_eq!(item.best_thumb(), Some("/thumb/parent"));
    }

    #[test]
    fn test_best_thumb_falls_back_to_grandparent() {
        let item = make_item(|i| {
            i.grandparent_thumb = Some("/thumb/gp".into());
        });
        assert_eq!(item.best_thumb(), Some("/thumb/gp"));
    }

    #[test]
    fn test_best_thumb_returns_none_when_all_absent() {
        let item = make_item(|_| {});
        assert_eq!(item.best_thumb(), None);
    }

    #[test]
    fn test_best_thumb_url_constructs_full_url() {
        let item = make_item(|i| {
            i.thumb = Some("/library/metadata/42/thumb/999".into());
        });
        let url = item.best_thumb_url("http://plex:32400", "tok123");
        assert_eq!(
            url,
            Some("http://plex:32400/library/metadata/42/thumb/999?X-Plex-Token=tok123".into())
        );
    }

    #[test]
    fn test_best_thumb_url_returns_none_without_thumb() {
        let item = make_item(|_| {});
        assert_eq!(item.best_thumb_url("http://plex:32400", "tok"), None);
    }

    // -- display_subtitle tests --

    #[test]
    fn test_display_subtitle_episode_with_indices() {
        let item = make_item(|i| {
            i.title = "Pilot".into();
            i.grandparent_title = Some("Breaking Bad".into());
            i.parent_index = Some(1);
            i.index = Some(1);
        });
        assert_eq!(item.display_subtitle(), Some("Breaking Bad S01E01".into()));
    }

    #[test]
    fn test_display_subtitle_episode_double_digit_indices() {
        let item = make_item(|i| {
            i.grandparent_title = Some("The Simpsons".into());
            i.parent_index = Some(12);
            i.index = Some(8);
        });
        assert_eq!(item.display_subtitle(), Some("The Simpsons S12E08".into()));
    }

    #[test]
    fn test_display_subtitle_episode_without_indices() {
        let item = make_item(|i| {
            i.grandparent_title = Some("Lost".into());
        });
        assert_eq!(item.display_subtitle(), Some("Lost".into()));
    }

    #[test]
    fn test_display_subtitle_episode_partial_index_only_season() {
        let item = make_item(|i| {
            i.grandparent_title = Some("Show".into());
            i.parent_index = Some(3);
            // index is None
        });
        assert_eq!(item.display_subtitle(), Some("Show".into()));
    }

    #[test]
    fn test_display_subtitle_episode_partial_index_only_episode() {
        let item = make_item(|i| {
            i.grandparent_title = Some("Show".into());
            i.index = Some(5);
            // parent_index is None
        });
        assert_eq!(item.display_subtitle(), Some("Show".into()));
    }

    #[test]
    fn test_display_subtitle_child_with_parent_title() {
        let item = make_item(|i| {
            i.parent_title = Some("Season 2".into());
        });
        assert_eq!(item.display_subtitle(), Some("Season 2".into()));
    }

    #[test]
    fn test_display_subtitle_movie_with_year() {
        let item = make_item(|i| {
            i.title = "The Matrix".into();
            i.year = Some(1999);
        });
        assert_eq!(item.display_subtitle(), Some("1999".into()));
    }

    #[test]
    fn test_display_subtitle_no_metadata_returns_none() {
        let item = make_item(|_| {});
        assert_eq!(item.display_subtitle(), None);
    }

    #[test]
    fn test_display_subtitle_grandparent_takes_priority_over_parent() {
        let item = make_item(|i| {
            i.grandparent_title = Some("Show".into());
            i.parent_title = Some("Season 1".into());
            i.parent_index = Some(1);
            i.index = Some(3);
            i.year = Some(2020);
        });
        assert_eq!(item.display_subtitle(), Some("Show S01E03".into()));
    }

    #[test]
    fn test_display_subtitle_parent_takes_priority_over_year() {
        let item = make_item(|i| {
            i.parent_title = Some("Season 1".into());
            i.year = Some(2020);
        });
        assert_eq!(item.display_subtitle(), Some("Season 1".into()));
    }

    // -- find_adjacent tests --

    fn make_episode(rk: &str, title: &str) -> MetadataItem {
        make_item(|i| {
            i.rating_key = rk.into();
            i.key = format!("/library/metadata/{}", rk);
            i.title = title.into();
        })
    }

    #[test]
    fn test_sort_popular_tracks_prefers_rating_then_views() {
        let a = make_item(|i| {
            i.rating_key = "a".into();
            i.user_rating = Some(7.0);
            i.view_count = Some(100);
        });
        let b = make_item(|i| {
            i.rating_key = "b".into();
            i.user_rating = Some(9.0);
            i.view_count = Some(5);
        });
        let c = make_item(|i| {
            i.rating_key = "c".into();
            i.user_rating = Some(7.0);
            i.view_count = Some(300);
        });
        let sorted = sort_popular_tracks(vec![a, b, c]);
        assert_eq!(sorted[0].rating_key, "b");
        assert_eq!(sorted[1].rating_key, "c");
        assert_eq!(sorted[2].rating_key, "a");
    }

    #[test]
    fn test_split_discography_items_groups_and_orders() {
        let album = make_item(|i| {
            i.rating_key = "album".into();
            i.title = "Main Album".into();
            i.media_type = Some("album".into());
            i.year = Some(2020);
        });
        let ep = make_item(|i| {
            i.rating_key = "ep".into();
            i.title = "EP One".into();
            i.media_type = Some("album".into());
            i.album_type = Some("ep".into());
            i.year = Some(2019);
        });
        let single = make_item(|i| {
            i.rating_key = "single".into();
            i.title = "Single One".into();
            i.media_type = Some("album".into());
            i.album_type = Some("single".into());
            i.year = Some(2022);
        });
        let (albums, eps_singles) = split_discography_items(vec![single, album, ep]);
        assert_eq!(albums.len(), 1);
        assert_eq!(albums[0].rating_key, "album");
        assert_eq!(eps_singles.len(), 2);
        assert_eq!(eps_singles[0].rating_key, "ep");
        assert_eq!(eps_singles[1].rating_key, "single");
    }

    #[test]
    fn test_find_adjacent_middle() {
        let episodes = vec![
            make_episode("10", "Ep1"),
            make_episode("11", "Ep2"),
            make_episode("12", "Ep3"),
        ];
        let result = find_adjacent(&episodes, "11").unwrap();
        assert_eq!(result.current.rating_key, "11");
        assert_eq!(result.previous.as_ref().unwrap().rating_key, "10");
        assert_eq!(result.next.as_ref().unwrap().rating_key, "12");
    }

    #[test]
    fn test_find_adjacent_first_has_no_prev() {
        let episodes = vec![
            make_episode("10", "Ep1"),
            make_episode("11", "Ep2"),
        ];
        let result = find_adjacent(&episodes, "10").unwrap();
        assert!(result.previous.is_none());
        assert_eq!(result.next.as_ref().unwrap().rating_key, "11");
    }

    #[test]
    fn test_find_adjacent_last_has_no_next() {
        let episodes = vec![
            make_episode("10", "Ep1"),
            make_episode("11", "Ep2"),
        ];
        let result = find_adjacent(&episodes, "11").unwrap();
        assert_eq!(result.previous.as_ref().unwrap().rating_key, "10");
        assert!(result.next.is_none());
    }

    #[test]
    fn test_find_adjacent_single_item() {
        let episodes = vec![make_episode("10", "Only")];
        let result = find_adjacent(&episodes, "10").unwrap();
        assert!(result.previous.is_none());
        assert!(result.next.is_none());
        assert_eq!(result.current.title, "Only");
    }

    #[test]
    fn test_find_adjacent_not_found() {
        let episodes = vec![make_episode("10", "Ep1")];
        assert!(find_adjacent(&episodes, "999").is_none());
    }

    #[test]
    fn test_find_adjacent_empty_list() {
        let episodes: Vec<MetadataItem> = vec![];
        assert!(find_adjacent(&episodes, "1").is_none());
    }

    #[test]
    fn test_collections_response_deserialize() {
        let json = r#"{
            "MediaContainer": {
                "size": 2,
                "Metadata": [
                    {
                        "ratingKey": "500",
                        "key": "/library/collections/500/children",
                        "title": "Marvel Cinematic Universe",
                        "type": "collection",
                        "summary": "All MCU films in release order.",
                        "thumb": "/library/collections/500/thumb/1234",
                        "childCount": 28
                    },
                    {
                        "ratingKey": "501",
                        "key": "/library/collections/501/children",
                        "title": "Star Wars",
                        "type": "collection",
                        "thumb": "/library/collections/501/thumb/5678"
                    }
                ]
            }
        }"#;
        let container: MediaContainer<MetadataItem> = serde_json::from_str(json).unwrap();
        let items = &container.media_container.metadata;
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].rating_key, "500");
        assert_eq!(items[0].title, "Marvel Cinematic Universe");
        assert_eq!(items[0].media_type, Some("collection".to_string()));
        assert_eq!(items[0].summary, Some("All MCU films in release order.".to_string()));
        assert_eq!(items[1].title, "Star Wars");
    }

    #[test]
    fn test_build_library_filter_query() {
        let filter = LibraryFilter {
            genre: Some("Sci-Fi".into()),
            year: Some("2024".into()),
            content_rating: Some("TV-MA".into()),
            resolution: Some("4k".into()),
            unwatched_only: true,
            audio_language: Some("en".into()),
            sort: Some("addedAt:desc".into()),
        };

        let query = build_library_filter_query(&filter);
        assert!(query.contains(&("genre".to_string(), "Sci-Fi".to_string())));
        assert!(query.contains(&("year".to_string(), "2024".to_string())));
        assert!(query.contains(&("contentRating".to_string(), "TV-MA".to_string())));
        assert!(query.contains(&("resolution".to_string(), "4k".to_string())));
        assert!(query.contains(&("unwatched".to_string(), "1".to_string())));
        assert!(query.contains(&("audioLanguage".to_string(), "en".to_string())));
        assert!(query.contains(&("sort".to_string(), "addedAt:desc".to_string())));
    }

    #[test]
    fn test_build_library_filter_query_empty() {
        let query = build_library_filter_query(&LibraryFilter::default());
        assert!(query.is_empty());
    }

    #[test]
    fn test_filter_option_deserialize_string_or_number_key() {
        let json = r#"{
            "MediaContainer": {
                "Directory": [
                    {"key": "action", "title": "Action"},
                    {"key": 2024, "title": "2024"}
                ]
            }
        }"#;
        let container: MediaContainer<FilterOption> = serde_json::from_str(json).unwrap();
        assert_eq!(container.media_container.directory.len(), 2);
        assert_eq!(container.media_container.directory[0].key, "action");
        assert_eq!(container.media_container.directory[1].key, "2024");
    }
}
