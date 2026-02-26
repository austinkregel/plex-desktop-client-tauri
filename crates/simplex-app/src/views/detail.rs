use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Button, Label, ListBox, ListBoxRow, Orientation, ScrolledWindow, Spinner};
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
            let (view_stack, prev) = {
                let s = state_back.lock().unwrap();
                (s.view_stack.clone(), s.previous_view.clone())
            };
            if let Some(vs) = view_stack {
                vs.set_visible_child_name(&prev.unwrap_or_else(|| "on-deck".to_string()));
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
                    let children = simplex_core::api::library::get_children(&bu, &tk, &key_c)
                        .await
                        .unwrap_or_default();
                    let artist_discography = if item.media_type.as_deref() == Some("artist") {
                        simplex_core::api::library::get_artist_discography(&bu, &tk, &key_c)
                            .await
                            .ok()
                    } else {
                        None
                    };
                    let _ = tx.send(Ok(DetailData {
                        item,
                        children,
                        artist_discography,
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
    base_url: String,
    token: String,
}

fn build_detail_ui(
    container: &GtkBox,
    data: &DetailData,
    state: &Arc<Mutex<AppState>>,
) {
    let item = &data.item;

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
        let children_title = match item.media_type.as_deref() {
            Some("show") => "Seasons",
            Some("season") => "Episodes",
            _ => "Items",
        };
        let children_label = Label::new(Some(children_title));
        children_label.add_css_class("title-2");
        children_label.set_halign(gtk4::Align::Start);
        children_label.set_margin_top(16);
        container.append(&children_label);

        let state_click = state.clone();
        let on_click: Rc<dyn Fn(&str)> = Rc::new(move |key: &str| {
            crate::window::navigate_to_detail(&state_click, key, "detail");
        });

        let grid = match item.media_type.as_deref() {
            Some("season") => PosterGrid::new_landscape(),
            Some("show") => PosterGrid::new(),
            _ => PosterGrid::new(),
        };
        grid.add_metadata_items_interactive(
            &data.children, &data.base_url, &data.token, on_click,
        );
        container.append(&grid.widget);
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

fn format_duration(ms: u64) -> String {
    let total_secs = ms / 1000;
    let h = total_secs / 3600;
    let m = (total_secs % 3600) / 60;
    if h > 0 {
        format!("{}h {}m", h, m)
    } else {
        format!("{}m", m)
    }
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().to_string() + chars.as_str(),
    }
}

fn truncate_text(text: &str, max_chars: usize) -> String {
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
