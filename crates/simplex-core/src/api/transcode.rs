/// Build a transcode (HLS) playback URL for a media item.
pub fn transcode_url(
    base_url: &str,
    token: &str,
    path: &str,
    session_id: &str,
) -> String {
    let base = base_url.trim_end_matches('/');
    format!(
        "{}/video/:/transcode/universal/start.m3u8?path={}&mediaIndex=0&partIndex=0\
         &protocol=hls&session={}&X-Plex-Token={}\
         &X-Plex-Product=Simplex&X-Plex-Client-Identifier=simplex-app",
        base,
        urlencoding::encode(path),
        urlencoding::encode(session_id),
        urlencoding::encode(token),
    )
}

/// Build a direct stream URL for a media part.
pub fn direct_stream_url(base_url: &str, token: &str, part_key: &str) -> String {
    let base = base_url.trim_end_matches('/');
    format!(
        "{}{}?X-Plex-Token={}",
        base,
        part_key,
        urlencoding::encode(token),
    )
}

/// Build a direct play URL using part ID.
pub fn direct_play_url(base_url: &str, token: &str, part_id: u64) -> String {
    let base = base_url.trim_end_matches('/');
    format!(
        "{}/library/parts/{}/file?X-Plex-Token={}",
        base,
        part_id,
        urlencoding::encode(token),
    )
}

/// Select the best playback URL for a metadata item.
/// Priority: direct stream (part key) > transcode (HLS).
pub fn playback_url_for_item(
    item: &super::library::MetadataItem,
    base_url: &str,
    token: &str,
    session_id: &str,
) -> Option<String> {
    let media = item.media.as_ref()?.first()?;
    let part = media.parts.as_ref()?.first()?;

    if let Some(ref key) = part.key {
        return Some(direct_stream_url(base_url, token, key));
    }

    Some(transcode_url(base_url, token, &item.key, session_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transcode_url() {
        let url = transcode_url("http://localhost:32400", "mytoken", "/library/metadata/123", "sess1");
        assert!(url.contains("start.m3u8"));
        assert!(url.contains("path=%2Flibrary%2Fmetadata%2F123"));
        assert!(url.contains("X-Plex-Token=mytoken"));
        assert!(url.contains("session=sess1"));
    }

    #[test]
    fn test_direct_stream_url() {
        let url = direct_stream_url("http://localhost:32400", "tok", "/library/parts/42/file.mkv");
        assert_eq!(url, "http://localhost:32400/library/parts/42/file.mkv?X-Plex-Token=tok");
    }

    #[test]
    fn test_direct_play_url() {
        let url = direct_play_url("http://localhost:32400", "tok", 42);
        assert_eq!(url, "http://localhost:32400/library/parts/42/file?X-Plex-Token=tok");
    }

    use super::super::library::{MetadataItem, MediaInfo, MediaPart};

    fn make_item_with_media(part_key: Option<&str>) -> MetadataItem {
        MetadataItem {
            rating_key: "1".into(),
            key: "/library/metadata/1".into(),
            guid: None,
            external_guids: vec![],
            title: "Test".into(),
            media_type: Some("movie".into()),
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
            media: Some(vec![MediaInfo {
                id: Some(100),
                duration: None,
                bitrate: None,
                width: None,
                height: None,
                video_codec: None,
                audio_codec: None,
                audio_channels: None,
                container: None,
                video_resolution: None,
                parts: Some(vec![MediaPart {
                    id: Some(200),
                    key: part_key.map(String::from),
                    duration: None,
                    file: None,
                    size: None,
                    container: None,
                    streams: None,
                }]),
            }]),
        }
    }

    #[test]
    fn test_playback_url_prefers_direct_stream() {
        let item = make_item_with_media(Some("/library/parts/200/file.mkv"));
        let url = playback_url_for_item(&item, "http://localhost:32400", "tok", "sess");
        assert_eq!(
            url.unwrap(),
            "http://localhost:32400/library/parts/200/file.mkv?X-Plex-Token=tok"
        );
    }

    #[test]
    fn test_playback_url_falls_back_to_transcode() {
        let item = make_item_with_media(None);
        let url = playback_url_for_item(&item, "http://localhost:32400", "tok", "sess").unwrap();
        assert!(url.contains("start.m3u8"));
        assert!(url.contains("path=%2Flibrary%2Fmetadata%2F1"));
    }

    #[test]
    fn test_playback_url_no_media_returns_none() {
        let mut item = make_item_with_media(Some("/key"));
        item.media = None;
        assert!(playback_url_for_item(&item, "http://x", "t", "s").is_none());
    }

    #[test]
    fn test_playback_url_empty_media_returns_none() {
        let mut item = make_item_with_media(Some("/key"));
        item.media = Some(vec![]);
        assert!(playback_url_for_item(&item, "http://x", "t", "s").is_none());
    }

    #[test]
    fn test_playback_url_no_parts_returns_none() {
        let mut item = make_item_with_media(Some("/key"));
        if let Some(ref mut media) = item.media {
            media[0].parts = None;
        }
        assert!(playback_url_for_item(&item, "http://x", "t", "s").is_none());
    }
}
