//! Pure state machine for player navigation.
//!
//! Tracks which fields change during transitions like "start playback",
//! "leave player", "enter PiP mode", and "navigate to detail". Extracted
//! from the GTK-dependent AppState so it can be unit-tested without a
//! display server.

/// Non-GUI portion of the application's player navigation state.
#[derive(Debug, Clone, Default)]
pub struct PlayerNavState {
    pub playback_uri: Option<String>,
    pub playback_title: Option<String>,
    pub playback_rating_key: Option<String>,
    pub current_item_key: Option<String>,
    pub previous_view: Option<String>,
}

impl PlayerNavState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Prepare state for playback. Sets playback URI/title/rating_key and
    /// defaults `previous_view` to `"detail"` if not already set.
    pub fn start_playback(&mut self, uri: &str, title: &str, rating_key: Option<&str>) {
        self.playback_uri = Some(uri.to_string());
        self.playback_title = Some(title.to_string());
        self.playback_rating_key = rating_key.map(String::from);
        if self.previous_view.is_none() {
            self.previous_view = Some("detail".to_string());
        }
    }

    /// Navigate to a detail page for a media item.
    pub fn navigate_to_detail(&mut self, rating_key: &str, from_view: &str) {
        self.current_item_key = Some(rating_key.to_string());
        self.previous_view = Some(from_view.to_string());
    }

    /// Fully leave the player: clears playback URI and rating key.
    /// Returns the view name to navigate back to.
    pub fn leave_player(&mut self) -> String {
        self.playback_uri = None;
        self.playback_rating_key = None;
        self.previous_view.clone().unwrap_or_else(|| "detail".into())
    }

    /// Enter PiP mode: the playback URI is kept (pipeline stays alive)
    /// but the UI navigates away from the player view.
    /// Returns the view name to navigate back to.
    pub fn enter_pip_mode(&mut self) -> String {
        self.previous_view.clone().unwrap_or_else(|| "detail".into())
    }

    /// Whether playback is currently intended (URI is set).
    pub fn is_playing(&self) -> bool {
        self.playback_uri.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_state() {
        let s = PlayerNavState::new();
        assert!(s.playback_uri.is_none());
        assert!(s.playback_title.is_none());
        assert!(s.playback_rating_key.is_none());
        assert!(s.current_item_key.is_none());
        assert!(s.previous_view.is_none());
        assert!(!s.is_playing());
    }

    // -- start_playback --

    #[test]
    fn test_start_playback_sets_uri_and_title() {
        let mut s = PlayerNavState::new();
        s.start_playback("http://plex/video.mp4", "My Movie", Some("42"));
        assert_eq!(s.playback_uri.as_deref(), Some("http://plex/video.mp4"));
        assert_eq!(s.playback_title.as_deref(), Some("My Movie"));
        assert_eq!(s.playback_rating_key.as_deref(), Some("42"));
        assert!(s.is_playing());
    }

    #[test]
    fn test_start_playback_defaults_previous_view_to_detail() {
        let mut s = PlayerNavState::new();
        assert!(s.previous_view.is_none());
        s.start_playback("http://plex/video.mp4", "Movie", None);
        assert_eq!(s.previous_view.as_deref(), Some("detail"));
    }

    #[test]
    fn test_start_playback_preserves_existing_previous_view() {
        let mut s = PlayerNavState::new();
        s.previous_view = Some("on-deck".to_string());
        s.start_playback("http://plex/video.mp4", "Movie", None);
        assert_eq!(s.previous_view.as_deref(), Some("on-deck"));
    }

    #[test]
    fn test_start_playback_overwrites_uri_on_second_call() {
        let mut s = PlayerNavState::new();
        s.start_playback("http://first.mp4", "First", Some("1"));
        s.start_playback("http://second.mp4", "Second", Some("2"));
        assert_eq!(s.playback_uri.as_deref(), Some("http://second.mp4"));
        assert_eq!(s.playback_title.as_deref(), Some("Second"));
        assert_eq!(s.playback_rating_key.as_deref(), Some("2"));
    }

    #[test]
    fn test_start_playback_with_no_rating_key() {
        let mut s = PlayerNavState::new();
        s.start_playback("http://plex/video.mp4", "Movie", None);
        assert!(s.playback_rating_key.is_none());
        assert!(s.is_playing());
    }

    // -- navigate_to_detail --

    #[test]
    fn test_navigate_to_detail_sets_key_and_previous() {
        let mut s = PlayerNavState::new();
        s.navigate_to_detail("12345", "on-deck");
        assert_eq!(s.current_item_key.as_deref(), Some("12345"));
        assert_eq!(s.previous_view.as_deref(), Some("on-deck"));
    }

    #[test]
    fn test_navigate_to_detail_overwrites_previous_view() {
        let mut s = PlayerNavState::new();
        s.previous_view = Some("library".to_string());
        s.navigate_to_detail("99", "search");
        assert_eq!(s.previous_view.as_deref(), Some("search"));
    }

    #[test]
    fn test_navigate_to_detail_does_not_affect_playback() {
        let mut s = PlayerNavState::new();
        s.start_playback("http://video.mp4", "V", None);
        s.navigate_to_detail("42", "on-deck");
        assert!(s.is_playing());
        assert_eq!(s.playback_uri.as_deref(), Some("http://video.mp4"));
    }

    // -- leave_player --

    #[test]
    fn test_leave_player_clears_uri_and_rating_key() {
        let mut s = PlayerNavState::new();
        s.start_playback("http://video.mp4", "V", Some("99"));
        assert!(s.is_playing());
        s.leave_player();
        assert!(!s.is_playing());
        assert!(s.playback_uri.is_none());
        assert!(s.playback_rating_key.is_none());
    }

    #[test]
    fn test_leave_player_preserves_title() {
        let mut s = PlayerNavState::new();
        s.start_playback("http://video.mp4", "Movie", None);
        s.leave_player();
        assert_eq!(s.playback_title.as_deref(), Some("Movie"));
    }

    #[test]
    fn test_leave_player_returns_previous_view() {
        let mut s = PlayerNavState::new();
        s.previous_view = Some("library".to_string());
        s.start_playback("http://video.mp4", "V", None);
        let view = s.leave_player();
        assert_eq!(view, "library");
    }

    #[test]
    fn test_leave_player_defaults_to_detail_when_no_previous() {
        let mut s = PlayerNavState::new();
        s.playback_uri = Some("x".to_string());
        s.previous_view = None;
        let view = s.leave_player();
        assert_eq!(view, "detail");
    }

    // -- enter_pip_mode --

    #[test]
    fn test_pip_mode_keeps_playback_uri() {
        let mut s = PlayerNavState::new();
        s.start_playback("http://video.mp4", "V", Some("10"));
        s.enter_pip_mode();
        assert!(s.is_playing());
        assert_eq!(s.playback_uri.as_deref(), Some("http://video.mp4"));
        assert_eq!(s.playback_rating_key.as_deref(), Some("10"));
    }

    #[test]
    fn test_pip_mode_returns_previous_view() {
        let mut s = PlayerNavState::new();
        s.previous_view = Some("on-deck".to_string());
        s.start_playback("http://video.mp4", "V", None);
        let view = s.enter_pip_mode();
        assert_eq!(view, "on-deck");
    }

    #[test]
    fn test_pip_mode_defaults_to_detail() {
        let mut s = PlayerNavState::new();
        s.playback_uri = Some("x".to_string());
        s.previous_view = None;
        let view = s.enter_pip_mode();
        assert_eq!(view, "detail");
    }

    #[test]
    fn test_leave_after_pip_clears_uri() {
        let mut s = PlayerNavState::new();
        s.start_playback("http://video.mp4", "V", Some("5"));
        s.enter_pip_mode();
        assert!(s.is_playing());
        s.leave_player();
        assert!(!s.is_playing());
        assert!(s.playback_rating_key.is_none());
    }

    // -- Full navigation flow --

    #[test]
    fn test_full_flow_browse_to_detail_to_play_to_pip_to_close() {
        let mut s = PlayerNavState::new();

        // User clicks item from on-deck
        s.navigate_to_detail("100", "on-deck");
        assert_eq!(s.current_item_key.as_deref(), Some("100"));
        assert_eq!(s.previous_view.as_deref(), Some("on-deck"));

        // User clicks play
        s.start_playback("http://plex/video/100", "Episode 1", Some("100"));
        assert!(s.is_playing());
        assert_eq!(s.previous_view.as_deref(), Some("on-deck"));
        assert_eq!(s.playback_rating_key.as_deref(), Some("100"));

        // User enables PiP
        let pip_dest = s.enter_pip_mode();
        assert_eq!(pip_dest, "on-deck");
        assert!(s.is_playing());

        // User closes PiP
        let leave_dest = s.leave_player();
        assert!(!s.is_playing());
        assert_eq!(leave_dest, "on-deck");
    }

    #[test]
    fn test_full_flow_play_then_escape() {
        let mut s = PlayerNavState::new();
        s.navigate_to_detail("50", "library");
        s.start_playback("http://plex/video/50", "Movie", Some("50"));
        assert!(s.is_playing());

        let dest = s.leave_player();
        assert!(!s.is_playing());
        assert_eq!(dest, "library");
    }

    #[test]
    fn test_play_from_detail_without_prior_navigation() {
        let mut s = PlayerNavState::new();
        s.start_playback("http://plex/video/1", "Title", None);
        assert_eq!(s.previous_view.as_deref(), Some("detail"));
        let dest = s.leave_player();
        assert_eq!(dest, "detail");
    }

    // -- Rating key tracking through episode switches --

    #[test]
    fn test_rating_key_updates_on_episode_switch() {
        let mut s = PlayerNavState::new();
        s.start_playback("http://plex/ep1", "Ep 1", Some("100"));
        assert_eq!(s.playback_rating_key.as_deref(), Some("100"));

        s.start_playback("http://plex/ep2", "Ep 2", Some("101"));
        assert_eq!(s.playback_rating_key.as_deref(), Some("101"));
        assert_eq!(s.playback_uri.as_deref(), Some("http://plex/ep2"));
    }

    #[test]
    fn test_rating_key_cleared_on_leave_after_episode_switch() {
        let mut s = PlayerNavState::new();
        s.navigate_to_detail("100", "on-deck");
        s.start_playback("http://plex/ep1", "Ep 1", Some("100"));
        s.start_playback("http://plex/ep2", "Ep 2", Some("101"));

        let dest = s.leave_player();
        assert!(s.playback_rating_key.is_none());
        assert!(!s.is_playing());
        assert_eq!(dest, "on-deck");
    }

    #[test]
    fn test_rating_key_persists_through_pip() {
        let mut s = PlayerNavState::new();
        s.start_playback("http://plex/ep1", "Ep 1", Some("200"));
        let _ = s.enter_pip_mode();
        assert_eq!(s.playback_rating_key.as_deref(), Some("200"));
        assert!(s.is_playing());
    }
}
