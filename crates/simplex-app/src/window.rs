use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Orientation};
use libadwaita::{
    Application, ApplicationWindow, HeaderBar, NavigationPage, NavigationSplitView, ViewStack,
};
use simplex_core::config;
use simplex_core::models::ServerConfig;
use std::sync::{Arc, Mutex};

use crate::player::pipeline::PlayerPipeline;
use crate::views;
use crate::widgets;

pub struct AppState {
    pub token: Option<String>,
    pub server: Option<ServerConfig>,
    pub client_id: String,
    pub view_stack: Option<ViewStack>,
    pub header_bar: Option<HeaderBar>,
    pub split_view: Option<NavigationSplitView>,
    pub window: Option<ApplicationWindow>,
    /// Rating key of the currently selected item for the detail page.
    pub current_item_key: Option<String>,
    /// View name to return to when pressing Back on the detail/player page.
    pub previous_view: Option<String>,
    /// Playback URI set by the detail page, consumed by the player view.
    pub playback_uri: Option<String>,
    /// Title of the currently playing media for display in the player.
    pub playback_title: Option<String>,
    /// Rating key of the currently playing item, used for metadata and episode nav.
    pub playback_rating_key: Option<String>,
    /// Resume offset in seconds. When set, the player seeks to this position on start.
    pub playback_offset: Option<f64>,
    /// Section key selected for library view (used by pinned library shortcuts).
    pub selected_library_key: Option<String>,
    /// Active shared playback pipeline used by full and mini player controls.
    pub playback_pipeline: Option<Arc<Mutex<PlayerPipeline>>>,
    /// Parent rating key of the currently displayed detail item, used for
    /// hierarchy-aware back navigation (episode→season→show→library).
    pub detail_parent_key: Option<String>,
}

impl AppState {
    pub fn new() -> Self {
        let token = simplex_core::keychain::get_auth_token().ok().flatten();
        let server = config::get_default_server().ok().flatten();
        let client_id = config::get_client_id()
            .ok()
            .flatten()
            .unwrap_or_else(|| {
                let id = uuid_string();
                let _ = config::set_client_id(id.clone());
                id
            });

        tracing::info!(
            "AppState: token={}, server={}, client_id={}",
            if token.is_some() { "present" } else { "none" },
            if server.is_some() { "present" } else { "none" },
            &client_id[..8.min(client_id.len())]
        );

        Self {
            token,
            server,
            client_id,
            view_stack: None,
            header_bar: None,
            split_view: None,
            window: None,
            current_item_key: None,
            previous_view: None,
            playback_uri: None,
            playback_title: None,
            playback_rating_key: None,
            playback_offset: None,
            selected_library_key: None,
            playback_pipeline: None,
            detail_parent_key: None,
        }
    }

    pub fn base_url(&self) -> Option<&str> {
        self.server.as_ref().map(|s| s.base_url.as_str())
    }

    #[cfg(test)]
    pub(crate) fn test_default() -> Self {
        Self {
            token: None,
            server: None,
            client_id: "test-client-id".to_string(),
            view_stack: None,
            header_bar: None,
            split_view: None,
            window: None,
            current_item_key: None,
            previous_view: None,
            playback_uri: None,
            playback_title: None,
            playback_rating_key: None,
            playback_offset: None,
            selected_library_key: None,
            playback_pipeline: None,
            detail_parent_key: None,
        }
    }
}

/// Navigate to the detail page for a given item.
pub fn navigate_to_detail(state: &Arc<Mutex<AppState>>, rating_key: &str, from_view: &str) {
    let view_stack = {
        let s = state.lock().unwrap();
        s.view_stack.clone()
    };

    let already_on_detail = view_stack
        .as_ref()
        .and_then(|v| v.visible_child_name())
        .as_deref()
        == Some("detail");

    let fallback = {
        let mut s = state.lock().unwrap();
        if !already_on_detail {
            s.previous_view = Some(from_view.to_string());
        }
        s.current_item_key = Some(rating_key.to_string());
        s.detail_parent_key = None;
        s.previous_view.clone()
    };

    if let Some(vs) = view_stack {
        if already_on_detail {
            // Force unmap/remap so connect_map fires with the new key.
            let tmp = fallback.unwrap_or_else(|| "on-deck".to_string());
            vs.set_visible_child_name(&tmp);
        }
        vs.set_visible_child_name("detail");
    }
}

/// Navigate to the player page with a given URI.
/// Hides the header bar and collapses the sidebar so the video fills the window.
pub fn navigate_to_player(
    state: &Arc<Mutex<AppState>>,
    uri: &str,
    title: &str,
    rating_key: Option<&str>,
    offset_secs: Option<f64>,
) {
    let (view_stack, header, split) = {
        let mut s = state.lock().unwrap();
        s.playback_uri = Some(uri.to_string());
        s.playback_title = Some(title.to_string());
        s.playback_rating_key = rating_key.map(String::from);
        s.playback_offset = offset_secs;
        if s.previous_view.is_none() {
            s.previous_view = Some("detail".to_string());
        }
        (s.view_stack.clone(), s.header_bar.clone(), s.split_view.clone())
    };
    if let Some(h) = header {
        h.set_visible(false);
    }
    if let Some(sv) = split {
        sv.set_show_content(true);
        sv.set_collapsed(true);
    }
    if let Some(vs) = view_stack {
        vs.set_visible_child_name("player");
    }
}

/// Restore the header bar, sidebar, and navigate back without stopping playback.
/// Used when entering PiP mode so the user can browse while video continues.
pub fn restore_chrome(state: &Arc<Mutex<AppState>>) {
    let (view_stack, header, split, prev, window) = {
        let s = state.lock().unwrap();
        let prev = s.previous_view.clone();
        (
            s.view_stack.clone(),
            s.header_bar.clone(),
            s.split_view.clone(),
            prev,
            s.window.clone(),
        )
    };
    if let Some(h) = header {
        h.set_visible(true);
    }
    if let Some(sv) = split {
        sv.set_collapsed(false);
    }
    if let Some(w) = window {
        if w.is_fullscreen() {
            w.set_fullscreened(false);
        }
    }
    if let Some(vs) = view_stack {
        let target = prev.unwrap_or_else(|| "detail".into());
        tracing::debug!("restore_chrome: switching to view '{}'", target);
        vs.set_visible_child_name(&target);
    }
}

/// Return from PiP to the full player view: hide chrome and switch to player.
/// The opposite of `restore_chrome`.
pub fn return_to_player(state: &Arc<Mutex<AppState>>) {
    let (view_stack, header, split) = {
        let s = state.lock().unwrap();
        (s.view_stack.clone(), s.header_bar.clone(), s.split_view.clone())
    };
    if let Some(h) = header {
        h.set_visible(false);
    }
    if let Some(sv) = split {
        sv.set_show_content(true);
        sv.set_collapsed(true);
    }
    if let Some(vs) = view_stack {
        vs.set_visible_child_name("player");
    }
}

/// Collapse from full player back to app chrome while keeping playback alive.
pub fn collapse_player(state: &Arc<Mutex<AppState>>) {
    restore_chrome(state);
}

/// Stop active playback and clear session state.
pub fn stop_playback_session(state: &Arc<Mutex<AppState>>) {
    let mut s = state.lock().unwrap();
    s.playback_uri = None;
    s.playback_title = None;
    s.playback_rating_key = None;
    s.playback_offset = None;
    s.playback_pipeline = None;
}

/// Fully leave the player: restore chrome and clear playback state.
pub fn leave_player(state: &Arc<Mutex<AppState>>) {
    stop_playback_session(state);
    restore_chrome(state);
}

pub(crate) fn uuid_string() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let bytes: [u8; 16] = rng.gen();
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_state() -> Arc<Mutex<AppState>> {
        Arc::new(Mutex::new(AppState::test_default()))
    }

    fn test_state_with_playback() -> Arc<Mutex<AppState>> {
        let state = test_state();
        {
            let mut s = state.lock().unwrap();
            s.playback_uri = Some("http://example.com/stream".to_string());
            s.playback_title = Some("Test Movie".to_string());
            s.playback_rating_key = Some("12345".to_string());
            s.playback_offset = Some(120.0);
        }
        state
    }

    #[test]
    fn test_base_url_none_without_server() {
        let state = AppState::test_default();
        assert!(state.base_url().is_none());
    }

    #[test]
    fn test_base_url_with_server() {
        let mut state = AppState::test_default();
        state.server = Some(ServerConfig {
            id: "test-id".to_string(),
            name: "Test".to_string(),
            base_url: "http://localhost:32400".to_string(),
            is_remote: false,
            machine_identifier: None,
        });
        assert_eq!(state.base_url(), Some("http://localhost:32400"));
    }

    #[test]
    fn test_uuid_string_length() {
        let uuid = uuid_string();
        assert_eq!(uuid.len(), 32);
    }

    #[test]
    fn test_uuid_string_is_hex() {
        let uuid = uuid_string();
        assert!(uuid.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_uuid_string_unique() {
        let a = uuid_string();
        let b = uuid_string();
        assert_ne!(a, b);
    }

    #[test]
    fn test_stop_playback_session_clears_fields() {
        let state = test_state_with_playback();
        stop_playback_session(&state);
        let s = state.lock().unwrap();
        assert!(s.playback_uri.is_none());
        assert!(s.playback_title.is_none());
        assert!(s.playback_rating_key.is_none());
        assert!(s.playback_offset.is_none());
        assert!(s.playback_pipeline.is_none());
    }

    #[test]
    fn test_navigate_to_detail_sets_key() {
        let state = test_state();
        navigate_to_detail(&state, "99", "library");
        let s = state.lock().unwrap();
        assert_eq!(s.current_item_key, Some("99".to_string()));
        assert_eq!(s.previous_view, Some("library".to_string()));
        assert!(s.detail_parent_key.is_none());
    }

    #[test]
    fn test_navigate_to_player_sets_state() {
        let state = test_state();
        navigate_to_player(&state, "http://stream", "Title", Some("42"), Some(30.0));
        let s = state.lock().unwrap();
        assert_eq!(s.playback_uri, Some("http://stream".to_string()));
        assert_eq!(s.playback_title, Some("Title".to_string()));
        assert_eq!(s.playback_rating_key, Some("42".to_string()));
        assert_eq!(s.playback_offset, Some(30.0));
        assert_eq!(s.previous_view, Some("detail".to_string()));
    }

    #[test]
    fn test_navigate_to_player_preserves_previous_view() {
        let state = test_state();
        {
            let mut s = state.lock().unwrap();
            s.previous_view = Some("library".to_string());
        }
        navigate_to_player(&state, "http://s", "T", None, None);
        let s = state.lock().unwrap();
        assert_eq!(s.previous_view, Some("library".to_string()));
    }

    #[test]
    fn test_leave_player_clears_playback() {
        let state = test_state_with_playback();
        leave_player(&state);
        let s = state.lock().unwrap();
        assert!(s.playback_uri.is_none());
        assert!(s.playback_title.is_none());
        assert!(s.playback_rating_key.is_none());
    }

    #[test]
    fn test_collapse_player_does_not_clear_playback() {
        let state = test_state_with_playback();
        collapse_player(&state);
        let s = state.lock().unwrap();
        assert!(s.playback_uri.is_some());
    }

    #[test]
    fn test_restore_chrome_no_widgets_does_not_panic() {
        let state = test_state();
        restore_chrome(&state);
    }

    #[test]
    fn test_return_to_player_no_widgets_does_not_panic() {
        let state = test_state();
        return_to_player(&state);
    }
}

pub fn build_window(app: &Application) {
    let state = Arc::new(Mutex::new(AppState::new()));

    let authenticated = {
        let s = state.lock().unwrap();
        s.token.is_some() && s.server.is_some()
    };

    let view_stack = ViewStack::new();

    {
        let mut s = state.lock().unwrap();
        s.view_stack = Some(view_stack.clone());
    }

    let login_page = views::login::build(state.clone());
    let on_deck_page = views::on_deck::build(state.clone());
    let library_page = views::library::build(state.clone());
    let search_page = views::search::build(state.clone());
    let playlists_page = views::playlists::build(state.clone());
    let collections_page = views::collections::build(state.clone());
    let detail_page = views::detail::build(state.clone());
    let player_page = views::player::build(state.clone());
    let settings_page = views::settings::build(state.clone());

    view_stack.add_titled(&login_page, Some("login"), "Login");
    view_stack.add_titled(&on_deck_page, Some("on-deck"), "On Deck");
    view_stack.add_titled(&library_page, Some("library"), "Library");
    view_stack.add_titled(&search_page, Some("search"), "Search");
    view_stack.add_titled(&playlists_page, Some("playlists"), "Playlists");
    view_stack.add_titled(&collections_page, Some("collections"), "Collections");
    view_stack.add_titled(&detail_page, Some("detail"), "Detail");
    view_stack.add_titled(&player_page, Some("player"), "Player");
    view_stack.add_titled(&settings_page, Some("settings"), "Settings");

    // Set the initial page BEFORE building the sidebar so the sidebar
    // doesn't override it via its row-selected signal.
    if authenticated {
        tracing::info!("Authenticated, showing On Deck");
        view_stack.set_visible_child_name("on-deck");
    } else {
        tracing::info!("Not authenticated, showing Login");
        view_stack.set_visible_child_name("login");
    }

    let sidebar = widgets::sidebar::build(&view_stack, state.clone());

    let content_box = GtkBox::new(Orientation::Vertical, 0);
    let header = HeaderBar::new();
    let mini_player = widgets::mini_player::build(state.clone());
    content_box.append(&header);
    content_box.append(&view_stack);
    content_box.append(&mini_player);

    let content_page = NavigationPage::builder()
        .child(&content_box)
        .title("Simplex")
        .build();

    let sidebar_page = NavigationPage::builder()
        .child(&sidebar)
        .title("Simplex")
        .build();

    let split_view = NavigationSplitView::builder()
        .sidebar(&sidebar_page)
        .content(&content_page)
        .build();

    let window = ApplicationWindow::builder()
        .application(app)
        .title("Simplex")
        .default_width(1200)
        .default_height(800)
        .content(&split_view)
        .build();

    {
        let mut s = state.lock().unwrap();
        s.header_bar = Some(header);
        s.split_view = Some(split_view);
        s.window = Some(window.clone());
    }

    window.present();
}
