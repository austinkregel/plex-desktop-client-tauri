use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Button, Label, ListBox, ListBoxRow, Orientation, ProgressBar, ScrolledWindow, Spinner};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use crate::widgets::entity_link::make_entity_link;
use crate::widgets::poster_grid::PosterGrid;
use crate::window::AppState;

pub fn build(state: Arc<Mutex<AppState>>) -> GtkBox {
    let container = GtkBox::new(Orientation::Vertical, 0);
    container.set_vexpand(true);

    let scroll = ScrolledWindow::new();
    scroll.set_vexpand(true);
    let content = GtkBox::new(Orientation::Vertical, 12);
    content.set_margin_start(24);
    content.set_margin_end(24);
    content.set_margin_top(16);
    content.set_margin_bottom(24);
    scroll.set_child(Some(&content));
    container.append(&scroll);

    let last_key: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));

    let state_c = state.clone();
    let content_c = content.clone();
    let last_key_c = last_key.clone();

    container.connect_map(move |_| {
        let current_key = {
            let s = state_c.lock().unwrap();
            s.current_item_key.clone()
        };
        let key = match current_key {
            Some(k) => k,
            None => return,
        };

        if last_key_c.borrow().as_deref() == Some(&key) {
            return;
        }
        *last_key_c.borrow_mut() = Some(key.clone());

        while let Some(child) = content_c.first_child() {
            content_c.remove(&child);
        }

        let back_row = GtkBox::new(Orientation::Horizontal, 8);
        let back_btn = Button::from_icon_name("go-previous-symbolic");
        back_btn.add_css_class("flat");
        let state_back = state_c.clone();
        back_btn.connect_clicked(move |_| {
            let (view_stack, parent_key, prev_view) = {
                let s = state_back.lock().unwrap();
                (
                    s.view_stack.clone(),
                    s.detail_parent_key.clone(),
                    s.previous_view.clone(),
                )
            };
            if let Some(vs) = view_stack {
                if let Some(key) = parent_key {
                    {
                        let mut s = state_back.lock().unwrap();
                        s.current_item_key = Some(key);
                        s.detail_parent_key = None;
                    }
                    let fallback = prev_view.unwrap_or_else(|| "on-deck".to_string());
                    vs.set_visible_child_name(&fallback);
                    vs.set_visible_child_name("detail");
                } else {
                    vs.set_visible_child_name(&prev_view.unwrap_or_else(|| "on-deck".to_string()));
                }
            }
        });
        back_row.append(&back_btn);
        let back_label = Label::new(Some("Back"));
        back_label.add_css_class("dim-label");
        back_row.append(&back_label);
        content_c.append(&back_row);

        let spinner = Spinner::new();
        spinner.set_spinning(true);
        spinner.set_halign(gtk4::Align::Center);
        spinner.set_vexpand(true);
        content_c.append(&spinner);

        let s = state_c.lock().unwrap();
        let token_url = s.token.clone().zip(s.base_url().map(String::from));
        drop(s);

        let (token, base_url) = match token_url {
            Some(pair) => pair,
            None => return,
        };

        let (tx, rx) = async_channel::unbounded::<Result<DetailData, String>>();

        let bu = base_url.clone();
        let tk = token.clone();
        let key_c = key.clone();
        crate::app::runtime().spawn(async move {
            match simplex_core::api::library::get_metadata(&bu, &tk, &key_c).await {
                Ok(item) => {
                    let children = match simplex_core::api::library::get_children(&bu, &tk, &key_c).await {
                        Ok(c) => c,
                        Err(e) => {
                            tracing::warn!("Failed to fetch children for {}: {}", key_c, e);
                            Vec::new()
                        }
                    };
                    let artist_discography = if item.media_type.as_deref() == Some("artist") {
                        simplex_core::api::library::get_artist_discography(&bu, &tk, &key_c)
                            .await
                            .ok()
                    } else {
                        None
                    };
                    let next_episode = if item.media_type.as_deref() == Some("show") {
                        simplex_core::api::library::get_next_episode(&bu, &tk, &key_c)
                            .await
                            .ok()
                            .flatten()
                    } else {
                        None
                    };
                    let _ = tx.send(Ok(DetailData {
                        item,
                        children,
                        artist_discography,
                        next_episode,
                        base_url: bu,
                        token: tk,
                    })).await;
                }
                Err(e) => {
                    let _ = tx.send(Err(e.to_string())).await;
                }
            }
        });

        let content2 = content_c.clone();
        let state2 = state_c.clone();
        glib::spawn_future_local(async move {
            if let Ok(result) = rx.recv().await {
                spinner.set_visible(false);

                match result {
                    Ok(data) => build_detail_ui(&content2, &data, &state2),
                    Err(err) => {
                        let label = Label::new(Some(&format!("Failed to load: {}", err)));
                        label.add_css_class("dim-label");
                        content2.append(&label);
                    }
                }
            }
        });
    });

    container
}

struct DetailData {
    item: simplex_core::api::library::MetadataItem,
    children: Vec<simplex_core::api::library::MetadataItem>,
    artist_discography: Option<simplex_core::api::library::ArtistDiscography>,
    next_episode: Option<simplex_core::api::library::MetadataItem>,
    base_url: String,
    token: String,
}

fn build_detail_ui(
    container: &GtkBox,
    data: &DetailData,
    state: &Arc<Mutex<AppState>>,
) {
    let item = &data.item;

    {
        let mut s = state.lock().unwrap();
        s.detail_parent_key = item.parent_rating_key.clone();
    }

    let title_btn = make_entity_link(&item.title, &item.rating_key, "detail", state.clone());
    title_btn.add_css_class("title-1");
    container.append(&title_btn);

    let mut meta_parts = Vec::new();
    if let Some(year) = item.year {
        meta_parts.push(year.to_string());
    }
    if let Some(ref mt) = item.media_type {
        meta_parts.push(capitalize(mt));
    }
    if let Some(dur) = item.duration {
        meta_parts.push(format_duration(dur));
    }
    if let Some(ref media_list) = item.media {
        if let Some(m) = media_list.first() {
            if let Some(ref res) = m.video_resolution {
                meta_parts.push(format!("{}p", res));
            }
            if let Some(ref codec) = m.video_codec {
                meta_parts.push(codec.to_uppercase());
            }
        }
    }
    if !meta_parts.is_empty() {
        let meta_label = Label::new(Some(&meta_parts.join(" \u{2022} ")));
        meta_label.add_css_class("dim-label");
        meta_label.set_halign(gtk4::Align::Start);
        container.append(&meta_label);
    }

    if let Some(sub) = item.display_subtitle() {
        let sub_label = Label::new(Some(&sub));
        sub_label.add_css_class("dim-label");
        sub_label.set_halign(gtk4::Align::Start);
        container.append(&sub_label);
    }

    // Make known hierarchy names drill into their own info pages.
    if let (Some(show), Some(show_key)) = (&item.grandparent_title, &item.grandparent_rating_key) {
        let show_row = GtkBox::new(Orientation::Horizontal, 4);
        let label = Label::new(Some("Show:"));
        label.add_css_class("dim-label");
        show_row.append(&label);
        show_row.append(&make_entity_link(show, show_key, "detail", state.clone()));
        container.append(&show_row);
    }
    if let (Some(album_or_season), Some(parent_key)) = (&item.parent_title, &item.parent_rating_key) {
        let parent_row = GtkBox::new(Orientation::Horizontal, 4);
        let prefix = match item.media_type.as_deref() {
            Some("track") => "Album:",
            Some("episode") => "Season:",
            _ => "Parent:",
        };
        let label = Label::new(Some(prefix));
        label.add_css_class("dim-label");
        parent_row.append(&label);
        parent_row.append(&make_entity_link(album_or_season, parent_key, "detail", state.clone()));
        container.append(&parent_row);
    }

    let is_playable = matches!(
        item.media_type.as_deref(),
        Some("movie") | Some("episode") | Some("clip") | Some("track") | None
    );
    let has_media = item.media.as_ref().map_or(false, |m| !m.is_empty());

    if is_playable && has_media {
        let button_row = GtkBox::new(Orientation::Horizontal, 8);
        button_row.set_margin_top(8);
        button_row.set_margin_bottom(8);

        let has_offset = item.view_offset.is_some() && item.view_offset.unwrap() > 0;
        let offset_secs = item.view_offset.map(|ms| ms as f64 / 1000.0);

        if has_offset {
            let resume_btn = Button::with_label(&format!(
                "Resume from {}", format_duration(item.view_offset.unwrap())
            ));
            resume_btn.add_css_class("suggested-action");
            resume_btn.add_css_class("pill");

            let state_resume = state.clone();
            let base_url_r = data.base_url.clone();
            let token_r = data.token.clone();
            let item_r = item.clone();
            resume_btn.connect_clicked(move |_| {
                if let Some(url) = simplex_core::api::transcode::playback_url_for_item(
                    &item_r, &base_url_r, &token_r, "simplex-session",
                ) {
                    crate::window::navigate_to_player(
                        &state_resume, &url, &item_r.title,
                        Some(&item_r.rating_key), offset_secs,
                    );
                }
            });
            button_row.append(&resume_btn);

            let beginning_btn = Button::with_label("Play from Beginning");
            beginning_btn.add_css_class("pill");

            let state_begin = state.clone();
            let base_url_b = data.base_url.clone();
            let token_b = data.token.clone();
            let item_b = item.clone();
            beginning_btn.connect_clicked(move |_| {
                if let Some(url) = simplex_core::api::transcode::playback_url_for_item(
                    &item_b, &base_url_b, &token_b, "simplex-session",
                ) {
                    crate::window::navigate_to_player(
                        &state_begin, &url, &item_b.title,
                        Some(&item_b.rating_key), None,
                    );
                }
            });
            button_row.append(&beginning_btn);
        } else {
            let play_btn = Button::with_label("Play");
            play_btn.add_css_class("suggested-action");
            play_btn.add_css_class("pill");

            let state_play = state.clone();
            let base_url = data.base_url.clone();
            let token = data.token.clone();
            let item_clone = item.clone();
            play_btn.connect_clicked(move |_| {
                if let Some(url) = simplex_core::api::transcode::playback_url_for_item(
                    &item_clone, &base_url, &token, "simplex-session",
                ) {
                    crate::window::navigate_to_player(
                        &state_play, &url, &item_clone.title,
                        Some(&item_clone.rating_key), None,
                    );
                }
            });
            button_row.append(&play_btn);
        }

        // Explicit parent-container navigation for episode/track detail pages.
        if let (Some(parent_title), Some(parent_key)) = (&item.parent_title, &item.parent_rating_key) {
            let label = match item.media_type.as_deref() {
                Some("episode") => Some("View Full Season"),
                Some("track") => Some("View Full Album"),
                _ => None,
            };
            if let Some(label) = label {
                let view_parent_btn = Button::with_label(label);
                view_parent_btn.add_css_class("pill");
                let state_parent = state.clone();
                let key_parent = parent_key.clone();
                view_parent_btn.connect_clicked(move |_| {
                    crate::window::navigate_to_detail(&state_parent, &key_parent, "detail");
                });
                view_parent_btn.set_tooltip_text(Some(&format!("Open {parent_title}")));
                button_row.append(&view_parent_btn);
            }
        }

        container.append(&button_row);
    } else if matches!(
        item.media_type.as_deref(),
        Some("show") | Some("album") | Some("artist")
    ) {
        let action_row = GtkBox::new(Orientation::Horizontal, 8);
        action_row.set_margin_top(8);
        action_row.set_margin_bottom(8);

        match item.media_type.as_deref() {
            Some("show") => {
                if let Some(ref next_ep) = data.next_episode {
                    let label_text = if next_ep.view_offset.unwrap_or(0) > 0 {
                        format!("Resume \"{}\"", next_ep.title)
                    } else {
                        format!("Play \"{}\"", next_ep.title)
                    };
                    let play_next_btn = Button::with_label(&label_text);
                    play_next_btn.add_css_class("suggested-action");
                    play_next_btn.add_css_class("pill");
                    let offset = next_ep.view_offset
                        .filter(|&o| o > 0)
                        .map(|ms| ms as f64 / 1000.0);
                    wire_play_button_with_offset(
                        &play_next_btn,
                        next_ep.clone(),
                        data.base_url.clone(),
                        data.token.clone(),
                        state.clone(),
                        offset,
                    );
                    action_row.append(&play_next_btn);
                }
            }
            Some("album") => {
                if let Some(first_track) = data.children.first() {
                    let play_btn = Button::with_label("Play Album");
                    play_btn.add_css_class("suggested-action");
                    play_btn.add_css_class("pill");
                    wire_play_button_for_item(
                        &play_btn,
                        first_track.clone(),
                        data.base_url.clone(),
                        data.token.clone(),
                        state.clone(),
                    );
                    action_row.append(&play_btn);
                }
                if data.children.len() > 1 {
                    let shuffle_btn = Button::with_label("Shuffle");
                    shuffle_btn.add_css_class("pill");
                    let tracks = data.children.clone();
                    let bu = data.base_url.clone();
                    let tk = data.token.clone();
                    let st = state.clone();
                    shuffle_btn.connect_clicked(move |_| {
                        let idx = pseudo_random_index(tracks.len());
                        wire_play_item_now(&tracks[idx], &bu, &tk, &st, None);
                    });
                    action_row.append(&shuffle_btn);
                }
            }
            Some("artist") => {
                if let Some(ref disco) = data.artist_discography {
                    if let Some(first_track) = disco.popular_tracks.first() {
                        let play_btn = Button::with_label("Play All");
                        play_btn.add_css_class("suggested-action");
                        play_btn.add_css_class("pill");
                        wire_play_button_for_item(
                            &play_btn,
                            first_track.clone(),
                            data.base_url.clone(),
                            data.token.clone(),
                            state.clone(),
                        );
                        action_row.append(&play_btn);
                    }
                    if disco.popular_tracks.len() > 1 {
                        let shuffle_btn = Button::with_label("Shuffle");
                        shuffle_btn.add_css_class("pill");
                        let all_tracks = disco.popular_tracks.clone();
                        let bu = data.base_url.clone();
                        let tk = data.token.clone();
                        let st = state.clone();
                        shuffle_btn.connect_clicked(move |_| {
                            let idx = pseudo_random_index(all_tracks.len());
                            wire_play_item_now(&all_tracks[idx], &bu, &tk, &st, None);
                        });
                        action_row.append(&shuffle_btn);
                    }
                }
            }
            _ => {}
        }

        if action_row.first_child().is_some() {
            container.append(&action_row);
        }
    }

    if let Some(ref summary) = item.summary {
        if !summary.is_empty() {
            let summary_text = if item.media_type.as_deref() == Some("artist") {
                truncate_text(summary, 800)
            } else {
                summary.clone()
            };
            let summary_label = Label::new(Some(&summary_text));
            summary_label.set_wrap(true);
            summary_label.set_halign(gtk4::Align::Start);
            summary_label.set_margin_top(8);
            summary_label.set_xalign(0.0);
            if item.media_type.as_deref() == Some("artist") {
                summary_label.set_lines(8);
                summary_label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
            }
            container.append(&summary_label);
        }
    }

    if let Some(disco) = &data.artist_discography {
        append_artist_sections(container, disco, &data.base_url, &data.token, state);
    } else if !data.children.is_empty() {
        match item.media_type.as_deref() {
            Some("season") => {
                let label = Label::new(Some("Episodes"));
                label.add_css_class("title-2");
                label.set_halign(gtk4::Align::Start);
                label.set_margin_top(16);
                container.append(&label);
                build_episode_list(container, &data.children, &data.base_url, &data.token, state);
            }
            Some("album") => {
                let label = Label::new(Some("Tracks"));
                label.add_css_class("title-2");
                label.set_halign(gtk4::Align::Start);
                label.set_margin_top(16);
                container.append(&label);
                build_track_list(container, &data.children, &data.base_url, &data.token, state);
            }
            Some("show") => {
                let label = Label::new(Some("Seasons"));
                label.add_css_class("title-2");
                label.set_halign(gtk4::Align::Start);
                label.set_margin_top(16);
                container.append(&label);
                build_seasons_grid(container, &data.children, &data.base_url, &data.token, state);
            }
            _ => {
                let label = Label::new(Some("Items"));
                label.add_css_class("title-2");
                label.set_halign(gtk4::Align::Start);
                label.set_margin_top(16);
                container.append(&label);

                let state_click = state.clone();
                let on_click: Rc<dyn Fn(&str)> = Rc::new(move |key: &str| {
                    crate::window::navigate_to_detail(&state_click, key, "detail");
                });
                let grid = PosterGrid::new();
                grid.add_metadata_items_interactive(
                    &data.children, &data.base_url, &data.token, on_click,
                );
                container.append(&grid.widget);
            }
        }
    }
}

fn append_artist_sections(
    container: &GtkBox,
    discography: &simplex_core::api::library::ArtistDiscography,
    base_url: &str,
    token: &str,
    state: &Arc<Mutex<AppState>>,
) {
    if !discography.popular_tracks.is_empty() {
        let popular_label = Label::new(Some("Popular"));
        popular_label.add_css_class("title-2");
        popular_label.set_halign(gtk4::Align::Start);
        popular_label.set_margin_top(16);
        container.append(&popular_label);

        let popular_list = ListBox::new();
        popular_list.set_selection_mode(gtk4::SelectionMode::None);
        for track in &discography.popular_tracks {
            let row = ListBoxRow::new();
            let row_box = GtkBox::new(Orientation::Horizontal, 6);
            row_box.set_margin_top(4);
            row_box.set_margin_bottom(4);
            row_box.set_margin_start(4);
            row_box.set_margin_end(4);
            row_box.append(&make_entity_link(&track.title, &track.rating_key, "detail", state.clone()));
            if let Some(album_key) = &track.parent_rating_key {
                let dot = Label::new(Some("•"));
                dot.add_css_class("dim-label");
                row_box.append(&dot);
                let album_name = track.parent_title.as_deref().unwrap_or("Album");
                row_box.append(&make_entity_link(album_name, album_key, "detail", state.clone()));
            } else if let Some(album) = &track.parent_title {
                let dim = Label::new(Some(&format!("• {}", album)));
                dim.add_css_class("dim-label");
                row_box.append(&dim);
            }
            let play_btn = Button::from_icon_name("media-playback-start-symbolic");
            play_btn.add_css_class("flat");
            play_btn.set_tooltip_text(Some("Play track"));
            play_btn.set_halign(gtk4::Align::End);
            row_box.append(&play_btn);
            wire_play_button_for_item(
                &play_btn,
                track.clone(),
                base_url.to_string(),
                token.to_string(),
                state.clone(),
            );
            row.set_child(Some(&row_box));
            popular_list.append(&row);
        }
        container.append(&popular_list);
    }

    if !discography.albums.is_empty() {
        let albums_label = Label::new(Some("Albums"));
        albums_label.add_css_class("title-2");
        albums_label.set_halign(gtk4::Align::Start);
        albums_label.set_margin_top(16);
        container.append(&albums_label);

        let click = {
            let state_click = state.clone();
            Rc::new(move |key: &str| crate::window::navigate_to_detail(&state_click, key, "detail"))
        };
        let grid = PosterGrid::new_square();
        grid.add_metadata_items_interactive(&discography.albums, base_url, token, click);
        container.append(&grid.widget);
    }

    if !discography.eps_and_singles.is_empty() {
        let eps_label = Label::new(Some("EPs & Singles"));
        eps_label.add_css_class("title-2");
        eps_label.set_halign(gtk4::Align::Start);
        eps_label.set_margin_top(16);
        container.append(&eps_label);

        let click = {
            let state_click = state.clone();
            Rc::new(move |key: &str| crate::window::navigate_to_detail(&state_click, key, "detail"))
        };
        let grid = PosterGrid::new_square();
        grid.add_metadata_items_interactive(&discography.eps_and_singles, base_url, token, click);
        container.append(&grid.widget);
    }
}

pub(crate) fn format_duration(ms: u64) -> String {
    let total_secs = ms / 1000;
    let h = total_secs / 3600;
    let m = (total_secs % 3600) / 60;
    if h > 0 {
        format!("{}h {}m", h, m)
    } else {
        format!("{}m", m)
    }
}

pub(crate) fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().to_string() + chars.as_str(),
    }
}

pub(crate) fn truncate_text(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut out = String::new();
    for c in text.chars().take(max_chars) {
        out.push(c);
    }
    out.push_str("...");
    out
}

fn build_episode_list(
    container: &GtkBox,
    episodes: &[simplex_core::api::library::MetadataItem],
    base_url: &str,
    token: &str,
    state: &Arc<Mutex<AppState>>,
) {
    let list = ListBox::new();
    list.set_selection_mode(gtk4::SelectionMode::None);
    list.add_css_class("boxed-list");

    for ep in episodes {
        let row = ListBoxRow::new();
        row.set_activatable(false);
        let row_box = GtkBox::new(Orientation::Horizontal, 12);
        row_box.set_margin_top(8);
        row_box.set_margin_bottom(8);
        row_box.set_margin_start(12);
        row_box.set_margin_end(12);

        if let Some(idx) = ep.index {
            let num_label = Label::new(Some(&format!("{}", idx)));
            num_label.add_css_class("dim-label");
            num_label.set_width_chars(3);
            num_label.set_xalign(1.0);
            row_box.append(&num_label);
        }

        let info_box = GtkBox::new(Orientation::Vertical, 2);
        info_box.set_hexpand(true);

        let title_link = make_entity_link(&ep.title, &ep.rating_key, "detail", state.clone());
        info_box.append(&title_link);

        let mut meta = Vec::new();
        if let Some(dur) = ep.duration {
            meta.push(format_duration(dur));
        }
        if let Some(ref date) = ep.originally_available_at {
            meta.push(date.clone());
        }
        if !meta.is_empty() {
            let meta_label = Label::new(Some(&meta.join(" \u{2022} ")));
            meta_label.add_css_class("dim-label");
            meta_label.add_css_class("caption");
            meta_label.set_halign(gtk4::Align::Start);
            info_box.append(&meta_label);
        }

        if let Some(ref summary) = ep.summary {
            if !summary.is_empty() {
                let summary_label = Label::new(Some(&truncate_text(summary, 120)));
                summary_label.add_css_class("dim-label");
                summary_label.add_css_class("caption");
                summary_label.set_halign(gtk4::Align::Start);
                summary_label.set_wrap(true);
                summary_label.set_lines(2);
                summary_label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
                summary_label.set_xalign(0.0);
                info_box.append(&summary_label);
            }
        }

        if let (Some(offset), Some(duration)) = (ep.view_offset, ep.duration) {
            if offset > 0 && duration > 0 {
                let progress = ProgressBar::new();
                progress.set_fraction(offset as f64 / duration as f64);
                progress.set_margin_top(4);
                progress.set_valign(gtk4::Align::Center);
                info_box.append(&progress);
            }
        }

        row_box.append(&info_box);

        if let Some(count) = ep.view_count {
            if count > 0 && ep.view_offset.unwrap_or(0) == 0 {
                let check = Label::new(Some("\u{2713}"));
                check.add_css_class("success");
                check.set_tooltip_text(Some("Watched"));
                check.set_valign(gtk4::Align::Center);
                row_box.append(&check);
            }
        }

        let play_btn = Button::from_icon_name("media-playback-start-symbolic");
        play_btn.add_css_class("flat");
        play_btn.set_tooltip_text(Some("Play episode"));
        play_btn.set_valign(gtk4::Align::Center);
        let offset = ep.view_offset
            .filter(|&o| o > 0)
            .map(|ms| ms as f64 / 1000.0);
        wire_play_button_with_offset(
            &play_btn,
            ep.clone(),
            base_url.to_string(),
            token.to_string(),
            state.clone(),
            offset,
        );
        row_box.append(&play_btn);

        row.set_child(Some(&row_box));
        list.append(&row);
    }

    container.append(&list);
}

fn build_track_list(
    container: &GtkBox,
    tracks: &[simplex_core::api::library::MetadataItem],
    base_url: &str,
    token: &str,
    state: &Arc<Mutex<AppState>>,
) {
    let list = ListBox::new();
    list.set_selection_mode(gtk4::SelectionMode::None);
    list.add_css_class("boxed-list");

    for track in tracks {
        let row = ListBoxRow::new();
        row.set_activatable(false);
        let row_box = GtkBox::new(Orientation::Horizontal, 8);
        row_box.set_margin_top(4);
        row_box.set_margin_bottom(4);
        row_box.set_margin_start(12);
        row_box.set_margin_end(12);

        if let Some(idx) = track.index {
            let num_label = Label::new(Some(&format!("{}", idx)));
            num_label.add_css_class("dim-label");
            num_label.set_width_chars(3);
            num_label.set_xalign(1.0);
            row_box.append(&num_label);
        }

        let title_link = make_entity_link(&track.title, &track.rating_key, "detail", state.clone());
        title_link.set_hexpand(true);
        title_link.set_halign(gtk4::Align::Start);
        row_box.append(&title_link);

        if let Some(dur) = track.duration {
            let dur_label = Label::new(Some(&format_track_duration(dur)));
            dur_label.add_css_class("dim-label");
            row_box.append(&dur_label);
        }

        let play_btn = Button::from_icon_name("media-playback-start-symbolic");
        play_btn.add_css_class("flat");
        play_btn.set_tooltip_text(Some("Play track"));
        play_btn.set_valign(gtk4::Align::Center);
        wire_play_button_for_item(
            &play_btn,
            track.clone(),
            base_url.to_string(),
            token.to_string(),
            state.clone(),
        );
        row_box.append(&play_btn);

        row.set_child(Some(&row_box));
        list.append(&row);
    }

    container.append(&list);
}

fn build_seasons_grid(
    container: &GtkBox,
    seasons: &[simplex_core::api::library::MetadataItem],
    base_url: &str,
    token: &str,
    state: &Arc<Mutex<AppState>>,
) {
    let grid = PosterGrid::new();
    let on_click: Rc<dyn Fn(&str)> = {
        let state_click = state.clone();
        Rc::new(move |key: &str| crate::window::navigate_to_detail(&state_click, key, "detail"))
    };

    for season in seasons {
        let thumb = season.best_thumb_url(base_url, token);
        let subtitle = match (season.viewed_leaf_count, season.leaf_count) {
            (Some(viewed), Some(total)) if total > 0 => {
                if viewed >= total {
                    Some(format!("All {} episodes watched", total))
                } else if viewed > 0 {
                    Some(format!("{} of {} watched", viewed, total))
                } else {
                    Some(format!("{} episodes", total))
                }
            }
            (_, Some(total)) => Some(format!("{} episodes", total)),
            _ => season.display_subtitle(),
        };
        grid.add_entry_interactive(
            &season.title,
            subtitle.as_deref(),
            thumb.as_deref(),
            &season.rating_key,
            &on_click,
        );
    }

    container.append(&grid.widget);
}

pub(crate) fn format_track_duration(ms: u64) -> String {
    let total_secs = ms / 1000;
    let m = total_secs / 60;
    let s = total_secs % 60;
    format!("{}:{:02}", m, s)
}

pub(crate) fn pseudo_random_index(len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as usize % len)
        .unwrap_or(0)
}

fn wire_play_button_with_offset(
    button: &Button,
    item: simplex_core::api::library::MetadataItem,
    base_url: String,
    token: String,
    state: Arc<Mutex<AppState>>,
    offset: Option<f64>,
) {
    button.connect_clicked(move |_| {
        wire_play_item_now(&item, &base_url, &token, &state, offset);
    });
}

fn wire_play_item_now(
    item: &simplex_core::api::library::MetadataItem,
    base_url: &str,
    token: &str,
    state: &Arc<Mutex<AppState>>,
    offset: Option<f64>,
) {
    if let Some(url) = simplex_core::api::transcode::playback_url_for_item(
        item, base_url, token, "simplex-session",
    ) {
        crate::window::navigate_to_player(state, &url, &item.title, Some(&item.rating_key), offset);
        return;
    }

    let (tx, rx) = async_channel::unbounded();
    let base_url_c = base_url.to_string();
    let token_c = token.to_string();
    let rating_key_c = item.rating_key.clone();
    crate::app::runtime().spawn(async move {
        let result = simplex_core::api::library::get_metadata(&base_url_c, &token_c, &rating_key_c).await;
        let _ = tx.send(result).await;
    });

    let state_c = state.clone();
    let base_url_ui = base_url.to_string();
    let token_ui = token.to_string();
    glib::spawn_future_local(async move {
        if let Ok(Ok(full_item)) = rx.recv().await {
            if let Some(url) = simplex_core::api::transcode::playback_url_for_item(
                &full_item, &base_url_ui, &token_ui, "simplex-session",
            ) {
                crate::window::navigate_to_player(
                    &state_c, &url, &full_item.title,
                    Some(&full_item.rating_key), offset,
                );
            }
        }
    });
}

fn wire_play_button_for_item(
    button: &Button,
    item: simplex_core::api::library::MetadataItem,
    base_url: String,
    token: String,
    state: Arc<Mutex<AppState>>,
) {
    button.connect_clicked(move |_| {
        if let Some(url) = simplex_core::api::transcode::playback_url_for_item(
            &item,
            &base_url,
            &token,
            "simplex-session",
        ) {
            crate::window::navigate_to_player(
                &state,
                &url,
                &item.title,
                Some(&item.rating_key),
                None,
            );
            return;
        }

        // Fallback: fetch full metadata first, then try playback URL generation again.
        let (tx, rx) = async_channel::unbounded();
        let base_url_c = base_url.clone();
        let token_c = token.clone();
        let rating_key_c = item.rating_key.clone();
        crate::app::runtime().spawn(async move {
            let result = simplex_core::api::library::get_metadata(&base_url_c, &token_c, &rating_key_c).await;
            let _ = tx.send(result).await;
        });

        let state_c = state.clone();
        let base_url_ui = base_url.clone();
        let token_ui = token.clone();
        glib::spawn_future_local(async move {
            if let Ok(Ok(full_item)) = rx.recv().await {
                if let Some(url) = simplex_core::api::transcode::playback_url_for_item(
                    &full_item,
                    &base_url_ui,
                    &token_ui,
                    "simplex-session",
                ) {
                    crate::window::navigate_to_player(
                        &state_c,
                        &url,
                        &full_item.title,
                        Some(&full_item.rating_key),
                        None,
                    );
                }
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_duration_hours_and_minutes() {
        assert_eq!(format_duration(7_200_000), "2h 0m");
    }

    #[test]
    fn test_format_duration_minutes_only() {
        assert_eq!(format_duration(300_000), "5m");
    }

    #[test]
    fn test_format_duration_zero() {
        assert_eq!(format_duration(0), "0m");
    }

    #[test]
    fn test_format_duration_mixed() {
        assert_eq!(format_duration(5_400_000), "1h 30m");
    }

    #[test]
    fn test_format_duration_seconds_ignored() {
        assert_eq!(format_duration(90_500), "1m");
    }

    #[test]
    fn test_capitalize_normal() {
        assert_eq!(capitalize("hello"), "Hello");
    }

    #[test]
    fn test_capitalize_empty() {
        assert_eq!(capitalize(""), "");
    }

    #[test]
    fn test_capitalize_already_upper() {
        assert_eq!(capitalize("World"), "World");
    }

    #[test]
    fn test_capitalize_single_char() {
        assert_eq!(capitalize("a"), "A");
    }

    #[test]
    fn test_truncate_text_short() {
        assert_eq!(truncate_text("hello", 10), "hello");
    }

    #[test]
    fn test_truncate_text_exact() {
        assert_eq!(truncate_text("hello", 5), "hello");
    }

    #[test]
    fn test_truncate_text_long() {
        assert_eq!(truncate_text("hello world", 5), "hello...");
    }

    #[test]
    fn test_truncate_text_empty() {
        assert_eq!(truncate_text("", 5), "");
    }

    #[test]
    fn test_format_track_duration_simple() {
        assert_eq!(format_track_duration(65_000), "1:05");
    }

    #[test]
    fn test_format_track_duration_zero() {
        assert_eq!(format_track_duration(0), "0:00");
    }

    #[test]
    fn test_format_track_duration_minutes() {
        assert_eq!(format_track_duration(180_000), "3:00");
    }

    #[test]
    fn test_format_track_duration_with_seconds() {
        assert_eq!(format_track_duration(245_000), "4:05");
    }

    #[test]
    fn test_pseudo_random_index_zero_len() {
        assert_eq!(pseudo_random_index(0), 0);
    }

    #[test]
    fn test_pseudo_random_index_one() {
        assert_eq!(pseudo_random_index(1), 0);
    }

    #[test]
    fn test_pseudo_random_index_in_range() {
        let idx = pseudo_random_index(100);
        assert!(idx < 100);
    }
}
