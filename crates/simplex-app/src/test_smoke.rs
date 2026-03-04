//! Headless GTK smoke tests for all view and widget build() functions.
//!
//! These tests verify that widget construction completes without panicking
//! when run under xvfb (headless display). They are bundled in a single
//! test to avoid GTK threading issues since GTK must be initialized on
//! the main thread and can only be initialized once per process.

#![cfg(test)]

use gtk4::prelude::*;
use std::sync::{Arc, Mutex, Once};

static GTK_INIT: Once = Once::new();

fn ensure_gtk_init() {
    GTK_INIT.call_once(|| {
        let _ = gtk4::init();
        gstreamer::init().ok();
        gstgtk4::plugin_register_static().ok();
    });
}

fn test_state() -> Arc<Mutex<crate::window::AppState>> {
    Arc::new(Mutex::new(crate::window::AppState::test_default()))
}

#[test]
fn smoke_test_all_views_and_widgets() {
    ensure_gtk_init();

    // If GTK init failed (no display), skip gracefully.
    if !gtk4::is_initialized() {
        eprintln!("GTK not initialized (no display?) — skipping smoke tests");
        return;
    }

    // --- Views ---

    let state = test_state();
    let _login = crate::views::login::build(state.clone());

    let state = test_state();
    let _on_deck = crate::views::on_deck::build(state.clone());

    let state = test_state();
    let _library = crate::views::library::build(state.clone());

    let state = test_state();
    let _search = crate::views::search::build(state.clone());

    let state = test_state();
    let _playlists = crate::views::playlists::build(state.clone());

    let state = test_state();
    let _collections = crate::views::collections::build(state.clone());

    let state = test_state();
    let _detail = crate::views::detail::build(state.clone());

    let state = test_state();
    let _settings = crate::views::settings::build(state.clone());

    let state = test_state();
    let _player = crate::views::player::build(state.clone());

    // --- Widgets ---

    let state = test_state();
    let _mini_player = crate::widgets::mini_player::build(state.clone());

    let state = test_state();
    let _entity_link = crate::widgets::entity_link::make_entity_link(
        "Test",
        "123",
        "library",
        state.clone(),
    );

    // PosterGrid
    let _grid = crate::widgets::poster_grid::PosterGrid::new();
    let _grid_sq = crate::widgets::poster_grid::PosterGrid::new_square();
    let _grid_ls = crate::widgets::poster_grid::PosterGrid::new_landscape();

    // MediaCard
    let _card = crate::widgets::media_card::MediaCard::new("Test Card", None, None);

    // Sidebar (needs a ViewStack)
    let state = test_state();
    let view_stack = libadwaita::ViewStack::new();
    let dummy = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    view_stack.add_titled(&dummy, Some("test"), "Test");
    let _sidebar = crate::widgets::sidebar::build(&view_stack, state.clone());

    // --- PlayerControls and PipWindow (require GStreamer pipeline) ---
    if let Ok(pipe) = crate::player::pipeline::PlayerPipeline::new() {
        let pipe = std::sync::Arc::new(std::sync::Mutex::new(pipe));

        // PlayerControls construction
        let ctrl = crate::player::controls::PlayerControls::new(&pipe);

        // Test set_metadata
        ctrl.set_metadata("Breaking Bad", "S5 · E14 — Ozymandias");
        assert_eq!(ctrl.show_label.text(), "Breaking Bad");
        assert_eq!(ctrl.episode_label.text(), "S5 · E14 — Ozymandias");

        // Test show/hide skip action
        ctrl.show_skip_action("Skip Intro");
        assert_eq!(ctrl.skip_action_button.label().unwrap(), "Skip Intro");
        ctrl.hide_skip_action();

        // Test show/hide up next
        ctrl.show_up_next("Next Episode", "S2 · E3");
        assert_eq!(ctrl.up_next_title.text(), "Next Episode");
        assert_eq!(ctrl.up_next_subtitle.text(), "S2 · E3");
        ctrl.set_up_next_countdown(3);
        assert_eq!(ctrl.up_next_countdown.text(), "Playing in 3...");
        ctrl.hide_up_next();

        // PipWindow construction
        let pip = crate::player::pip::PipWindow::new(&pipe);
        assert!(!pip.is_visible());
        pip.on_close(|| {});
        pip.on_return(|| {});

        // Quick settings build
        let (_btn, _popover) = crate::player::quick_settings::build(&pipe);
    } else {
        eprintln!("GStreamer pipeline unavailable — skipping controls/pip smoke tests");
    }
}
