use thiserror::Error;

const LIBRARY_IDENTIFIER: &str = "com.plexapp.plugins.library";

#[derive(Debug, Error)]
pub enum PlaybackError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
}

#[derive(Debug, Clone, Copy)]
pub enum TimelineState {
    Playing,
    Paused,
    Stopped,
}

impl TimelineState {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Playing => "playing",
            Self::Paused => "paused",
            Self::Stopped => "stopped",
        }
    }
}

/// Send playback progress updates to Plex timeline.
pub async fn update_timeline(
    base_url: &str,
    token: &str,
    rating_key: &str,
    key: &str,
    time_ms: u64,
    duration_ms: Option<u64>,
    state: TimelineState,
) -> Result<(), PlaybackError> {
    let client = super::plex_client(token)?;
    let url = format!("{}/:/timeline", base_url.trim_end_matches('/'));

    let mut query = vec![
        ("ratingKey".to_string(), rating_key.to_string()),
        ("key".to_string(), key.to_string()),
        ("identifier".to_string(), LIBRARY_IDENTIFIER.to_string()),
        ("time".to_string(), time_ms.to_string()),
        ("state".to_string(), state.as_str().to_string()),
    ];
    if let Some(duration_ms) = duration_ms {
        query.push(("duration".to_string(), duration_ms.to_string()));
    }

    client
        .get(url)
        .query(&query)
        .send()
        .await?
        .error_for_status()?;
    Ok(())
}

/// Mark an item as watched.
pub async fn scrobble(base_url: &str, token: &str, rating_key: &str) -> Result<(), PlaybackError> {
    let client = super::plex_client(token)?;
    let url = format!("{}/:/scrobble", base_url.trim_end_matches('/'));
    let query = vec![
        ("key".to_string(), rating_key.to_string()),
        ("identifier".to_string(), LIBRARY_IDENTIFIER.to_string()),
    ];

    client
        .get(url)
        .query(&query)
        .send()
        .await?
        .error_for_status()?;
    crate::cache::invalidate_prefix(&format!("library:get_metadata:{base_url}:{rating_key}"));
    crate::cache::invalidate_prefix(&format!("library:get_section_items_filtered:{base_url}:"));
    crate::cache::invalidate_prefix(&format!("hubs:get_hubs:{base_url}"));
    Ok(())
}

/// Mark an item as unwatched.
pub async fn unscrobble(
    base_url: &str,
    token: &str,
    rating_key: &str,
) -> Result<(), PlaybackError> {
    let client = super::plex_client(token)?;
    let url = format!("{}/:/unscrobble", base_url.trim_end_matches('/'));
    let query = vec![
        ("key".to_string(), rating_key.to_string()),
        ("identifier".to_string(), LIBRARY_IDENTIFIER.to_string()),
    ];

    client
        .get(url)
        .query(&query)
        .send()
        .await?
        .error_for_status()?;
    crate::cache::invalidate_prefix(&format!("library:get_metadata:{base_url}:{rating_key}"));
    crate::cache::invalidate_prefix(&format!("library:get_section_items_filtered:{base_url}:"));
    crate::cache::invalidate_prefix(&format!("hubs:get_hubs:{base_url}"));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_timeline_state_playing_as_str() {
        assert_eq!(TimelineState::Playing.as_str(), "playing");
    }

    #[test]
    fn test_timeline_state_paused_as_str() {
        assert_eq!(TimelineState::Paused.as_str(), "paused");
    }

    #[test]
    fn test_timeline_state_stopped_as_str() {
        assert_eq!(TimelineState::Stopped.as_str(), "stopped");
    }

    #[test]
    fn test_timeline_state_is_copy() {
        let state = TimelineState::Playing;
        let copy = state;
        assert_eq!(state.as_str(), copy.as_str());
    }

    #[test]
    fn test_timeline_state_debug() {
        let dbg = format!("{:?}", TimelineState::Paused);
        assert!(dbg.contains("Paused"));
    }

    #[test]
    fn test_all_timeline_states_covered() {
        let states = [
            TimelineState::Playing,
            TimelineState::Paused,
            TimelineState::Stopped,
        ];
        let expected = ["playing", "paused", "stopped"];
        for (state, exp) in states.iter().zip(expected.iter()) {
            assert_eq!(state.as_str(), *exp);
        }
    }
}
