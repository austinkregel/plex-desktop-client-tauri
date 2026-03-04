use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Button, Label, Orientation, Picture, Scale};
use simplex_core::api::library::{self, MetadataItem};
use simplex_core::api::transcode;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use crate::window::AppState;

fn format_time(seconds: f64) -> String {
    simplex_core::ui_utils::format_time(seconds.max(0.0))
}

#[derive(Clone)]
struct NavTarget {
    uri: String,
    title: String,
    rating_key: String,
}

#[derive(Clone, Default)]
struct MiniContext {
    subtitle: Option<String>,
    artwork_url: Option<String>,
    prev: Option<NavTarget>,
    next: Option<NavTarget>,
}

fn switch_to_target(state: &Arc<Mutex<AppState>>, target: &NavTarget) {
    let pipe = {
        let mut s = state.lock().unwrap();
        s.playback_uri = Some(target.uri.clone());
        s.playback_title = Some(target.title.clone());
        s.playback_rating_key = Some(target.rating_key.clone());
        s.playback_offset = None;
        s.playback_pipeline.clone()
    };
    if let Some(pipe) = pipe {
        let mut p = pipe.lock().unwrap();
        p.stop();
        p.set_uri(&target.uri);
        p.play();
    }
}

pub(crate) fn track_subtitle(item: &MetadataItem) -> Option<String> {
    let artist = item.grandparent_title.as_deref().unwrap_or_default();
    let album = item.parent_title.as_deref().unwrap_or_default();
    match (artist.is_empty(), album.is_empty()) {
        (false, false) => Some(format!("{artist} - {album}")),
        (false, true) => Some(artist.to_string()),
        (true, false) => Some(album.to_string()),
        (true, true) => None,
    }
}

async fn resolve_context(base_url: &str, token: &str, rating_key: &str) -> MiniContext {
    let item = match library::get_metadata(base_url, token, rating_key).await {
        Ok(item) => item,
        Err(_) => return MiniContext::default(),
    };

    let subtitle = if item.media_type.as_deref() == Some("track") {
        track_subtitle(&item)
    } else {
        item.display_subtitle()
    };
    let artwork_url = item.best_thumb_url(base_url, token);
    let mut ctx = MiniContext {
        subtitle,
        artwork_url,
        prev: None,
        next: None,
    };

    match item.media_type.as_deref() {
        Some("episode") => {
            if let Ok(adjacent) = library::get_adjacent_episodes(base_url, token, rating_key).await {
                if let Some(prev_item) = adjacent.previous {
                    if let Some(uri) =
                        transcode::playback_url_for_item(&prev_item, base_url, token, "simplex-session")
                    {
                        ctx.prev = Some(NavTarget {
                            uri,
                            title: prev_item.title,
                            rating_key: prev_item.rating_key,
                        });
                    }
                }
                if let Some(next_item) = adjacent.next {
                    if let Some(uri) =
                        transcode::playback_url_for_item(&next_item, base_url, token, "simplex-session")
                    {
                        ctx.next = Some(NavTarget {
                            uri,
                            title: next_item.title,
                            rating_key: next_item.rating_key,
                        });
                    }
                }
            }
        }
        Some("track") => {
            if let Some(parent_key) = item.parent_rating_key.as_deref() {
                if let Ok(mut tracks) = library::get_children(base_url, token, parent_key).await {
                    tracks.retain(|t| t.media_type.as_deref() == Some("track"));
                    tracks.sort_by_key(|t| t.index.unwrap_or(0));
                    if let Some(current_idx) = tracks.iter().position(|t| t.rating_key == item.rating_key) {
                        if current_idx > 0 {
                            let prev_item = tracks[current_idx - 1].clone();
                            if let Some(uri) =
                                transcode::playback_url_for_item(&prev_item, base_url, token, "simplex-session")
                            {
                                ctx.prev = Some(NavTarget {
                                    uri,
                                    title: prev_item.title,
                                    rating_key: prev_item.rating_key,
                                });
                            }
                        }
                        if current_idx + 1 < tracks.len() {
                            let next_item = tracks[current_idx + 1].clone();
                            if let Some(uri) =
                                transcode::playback_url_for_item(&next_item, base_url, token, "simplex-session")
                            {
                                ctx.next = Some(NavTarget {
                                    uri,
                                    title: next_item.title,
                                    rating_key: next_item.rating_key,
                                });
                            }
                        }
                    }
                }
            }
        }
        _ => {}
    }

    ctx
}

fn load_artwork_async(url: &str, picture: &Picture) {
    let url = url.to_string();
    let pic = picture.clone();
    let (tx, rx) = async_channel::unbounded::<Vec<u8>>();

    crate::app::runtime().spawn(async move {
        let resp = match reqwest::get(&url).await {
            Ok(r) => r,
            Err(_) => return,
        };
        if !resp.status().is_success() {
            return;
        }
        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if !simplex_core::ui_utils::is_image_content_type(content_type) {
            return;
        }
        if let Ok(bytes) = resp.bytes().await {
            let _ = tx.send(bytes.to_vec()).await;
        }
    });

    glib::spawn_future_local(async move {
        if let Ok(bytes) = rx.recv().await {
            if pic.parent().is_none() {
                return;
            }
            let g_bytes = glib::Bytes::from(&bytes);
            if let Ok(texture) = gdk4::Texture::from_bytes(&g_bytes) {
                pic.set_paintable(Some(&texture));
            }
        }
    });
}

pub fn build(state: Arc<Mutex<AppState>>) -> GtkBox {
    let bar = GtkBox::new(Orientation::Horizontal, 8);
    bar.add_css_class("toolbar");
    bar.add_css_class("osd");
    bar.set_margin_start(8);
    bar.set_margin_end(8);
    bar.set_margin_bottom(8);
    bar.set_margin_top(4);
    bar.set_visible(false);

    let artwork = Picture::new();
    artwork.set_size_request(44, 44);
    artwork.set_content_fit(gtk4::ContentFit::Cover);
    artwork.add_css_class("card");
    bar.append(&artwork);

    let meta_box = GtkBox::new(Orientation::Vertical, 2);
    meta_box.set_hexpand(true);
    meta_box.set_halign(gtk4::Align::Fill);
    bar.append(&meta_box);

    let title = Label::new(Some(""));
    title.set_halign(gtk4::Align::Start);
    title.add_css_class("heading");
    title.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    title.set_max_width_chars(48);
    meta_box.append(&title);

    let subtitle = Label::new(Some(""));
    subtitle.set_halign(gtk4::Align::Start);
    subtitle.add_css_class("dim-label");
    subtitle.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    subtitle.set_max_width_chars(56);
    subtitle.set_visible(false);
    meta_box.append(&subtitle);

    let position = Label::new(Some("0:00"));
    position.add_css_class("numeric");
    bar.append(&position);

    let seek = Scale::with_range(Orientation::Horizontal, 0.0, 100.0, 1.0);
    seek.set_hexpand(true);
    seek.set_draw_value(false);
    seek.set_size_request(200, -1);
    bar.append(&seek);
    let seek_updating = Rc::new(Cell::new(false));
    let nav_cache: Rc<RefCell<MiniContext>> = Rc::new(RefCell::new(MiniContext::default()));
    let last_rating_key: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    let last_artwork_url: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));

    let duration = Label::new(Some("0:00"));
    duration.add_css_class("numeric");
    bar.append(&duration);

    let prev = Button::from_icon_name("media-skip-backward-symbolic");
    prev.add_css_class("flat");
    prev.set_tooltip_text(Some("Previous"));
    prev.set_sensitive(false);
    bar.append(&prev);

    let play_pause = Button::from_icon_name("media-playback-start-symbolic");
    play_pause.add_css_class("flat");
    play_pause.set_tooltip_text(Some("Play/Pause"));
    bar.append(&play_pause);

    let next = Button::from_icon_name("media-skip-forward-symbolic");
    next.add_css_class("flat");
    next.set_tooltip_text(Some("Next"));
    next.set_sensitive(false);
    bar.append(&next);

    let expand = Button::from_icon_name("view-fullscreen-symbolic");
    expand.add_css_class("flat");
    expand.set_tooltip_text(Some("Expand player"));
    bar.append(&expand);

    let stop = Button::from_icon_name("media-playback-stop-symbolic");
    stop.add_css_class("flat");
    stop.set_tooltip_text(Some("Stop playback"));
    bar.append(&stop);

    {
        let state_prev = state.clone();
        let nav_prev = nav_cache.clone();
        prev.connect_clicked(move |_| {
            let target = nav_prev.borrow().prev.clone();
            if let Some(target) = target {
                switch_to_target(&state_prev, &target);
            }
        });
    }

    {
        let state_pp = state.clone();
        play_pause.connect_clicked(move |_| {
            let pipe = {
                state_pp.lock().unwrap().playback_pipeline.clone()
            };
            if let Some(pipe) = pipe {
                let p = pipe.lock().unwrap();
                p.toggle_play_pause();
            }
        });
    }

    {
        let state_next = state.clone();
        let nav_next = nav_cache.clone();
        next.connect_clicked(move |_| {
            let target = nav_next.borrow().next.clone();
            if let Some(target) = target {
                switch_to_target(&state_next, &target);
            }
        });
    }

    {
        let state_seek = state.clone();
        let seek_updating_flag = seek_updating.clone();
        seek.connect_value_changed(move |scale| {
            if seek_updating_flag.get() {
                return;
            }
            let pipe = {
                state_seek.lock().unwrap().playback_pipeline.clone()
            };
            if let Some(pipe) = pipe {
                let p = pipe.lock().unwrap();
                if let Some(dur) = p.duration() {
                    let frac = scale.value() / 100.0;
                    p.seek(frac * dur);
                }
            }
        });
    }

    {
        let state_expand = state.clone();
        expand.connect_clicked(move |_| {
            crate::window::return_to_player(&state_expand);
        });
    }

    {
        let state_stop = state.clone();
        stop.connect_clicked(move |_| {
            if let Some(pipe) = state_stop.lock().unwrap().playback_pipeline.clone() {
                pipe.lock().unwrap().stop();
            }
            crate::window::stop_playback_session(&state_stop);
        });
    }

    {
        let state_tick = state.clone();
        let bar_tick = bar.clone();
        let title_tick = title.clone();
        let subtitle_tick = subtitle.clone();
        let artwork_tick = artwork.clone();
        let pos_tick = position.clone();
        let dur_tick = duration.clone();
        let seek_tick = seek.clone();
        let seek_updating_tick = seek_updating.clone();
        let btn_tick = play_pause.clone();
        let prev_tick = prev.clone();
        let next_tick = next.clone();
        let nav_tick = nav_cache.clone();
        let last_rk_tick = last_rating_key.clone();
        let last_art_tick = last_artwork_url.clone();
        glib::timeout_add_local(std::time::Duration::from_millis(500), move || {
            let (pipe, playback_title, playback_uri, playback_rating_key, view_name, token, base_url) = {
                let s = state_tick.lock().unwrap();
                (
                    s.playback_pipeline.clone(),
                    s.playback_title.clone().unwrap_or_default(),
                    s.playback_uri.clone(),
                    s.playback_rating_key.clone(),
                    s.view_stack
                        .as_ref()
                        .and_then(|vs| vs.visible_child_name().map(|n| n.to_string()))
                        .unwrap_or_default(),
                    s.token.clone(),
                    s.base_url().map(String::from),
                )
            };

            let has_session = pipe.is_some() && playback_uri.is_some();
            let should_show = has_session && view_name != "player";
            if bar_tick.is_visible() != should_show {
                tracing::debug!(
                    "mini_player visibility {} -> {} (pipeline={}, uri={}, view={})",
                    bar_tick.is_visible(),
                    should_show,
                    pipe.is_some(),
                    playback_uri.is_some(),
                    view_name,
                );
            }
            bar_tick.set_visible(should_show);
            if !has_session {
                *last_rk_tick.borrow_mut() = None;
                *last_art_tick.borrow_mut() = None;
                *nav_tick.borrow_mut() = MiniContext::default();
                subtitle_tick.set_visible(false);
                subtitle_tick.set_text("");
                artwork_tick.set_paintable(Option::<&gdk4::Texture>::None);
                prev_tick.set_sensitive(false);
                next_tick.set_sensitive(false);
                return glib::ControlFlow::Continue;
            }

            title_tick.set_text(&playback_title);

            if let (Some(rk), Some(tk), Some(bu)) = (playback_rating_key, token, base_url) {
                if last_rk_tick.borrow().as_deref() != Some(rk.as_str()) {
                    *last_rk_tick.borrow_mut() = Some(rk.clone());
                    let (tx, rx) = async_channel::unbounded();
                    crate::app::runtime().spawn(async move {
                        let ctx = resolve_context(&bu, &tk, &rk).await;
                        let _ = tx.send(ctx).await;
                    });
                    let nav_update = nav_tick.clone();
                    let subtitle_update = subtitle_tick.clone();
                    let prev_update = prev_tick.clone();
                    let next_update = next_tick.clone();
                    let artwork_update = artwork_tick.clone();
                    let last_art_update = last_art_tick.clone();
                    glib::spawn_future_local(async move {
                        if let Ok(ctx) = rx.recv().await {
                            *nav_update.borrow_mut() = ctx.clone();
                            if let Some(text) = ctx.subtitle {
                                subtitle_update.set_text(&text);
                                subtitle_update.set_visible(true);
                            } else {
                                subtitle_update.set_visible(false);
                                subtitle_update.set_text("");
                            }
                            prev_update.set_sensitive(ctx.prev.is_some());
                            next_update.set_sensitive(ctx.next.is_some());
                            if let Some(url) = ctx.artwork_url {
                                if last_art_update.borrow().as_deref() != Some(url.as_str()) {
                                    *last_art_update.borrow_mut() = Some(url.clone());
                                    load_artwork_async(&url, &artwork_update);
                                }
                            } else {
                                *last_art_update.borrow_mut() = None;
                                artwork_update.set_paintable(Option::<&gdk4::Texture>::None);
                            }
                        }
                    });
                }
            }

            if let Some(pipe) = pipe {
                let p = pipe.lock().unwrap();
                let pos = p.position().unwrap_or(0.0);
                let dur = p.duration().unwrap_or(0.0);
                let can_seek = dur > 0.0;
                pos_tick.set_visible(can_seek);
                seek_tick.set_visible(can_seek);
                dur_tick.set_visible(can_seek);
                if can_seek {
                    pos_tick.set_text(&format_time(pos));
                    dur_tick.set_text(&format_time(dur));
                    seek_updating_tick.set(true);
                    seek_tick.set_value((pos / dur * 100.0).clamp(0.0, 100.0));
                    seek_updating_tick.set(false);
                }
                if p.is_playing() {
                    btn_tick.set_icon_name("media-playback-pause-symbolic");
                } else {
                    btn_tick.set_icon_name("media-playback-start-symbolic");
                }
            }
            glib::ControlFlow::Continue
        });
    }

    bar
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_item(
        grandparent: Option<&str>,
        parent: Option<&str>,
    ) -> MetadataItem {
        MetadataItem {
            rating_key: "1".to_string(),
            key: "/library/metadata/1".to_string(),
            title: "Track".to_string(),
            media_type: Some("track".to_string()),
            grandparent_title: grandparent.map(String::from),
            parent_title: parent.map(String::from),
            summary: None,
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
            parent_index: None,
            index: None,
            leaf_count: None,
            viewed_leaf_count: None,
            media: None,
            markers: vec![],
            year: None,
        }
    }

    #[test]
    fn test_track_subtitle_both() {
        let item = make_item(Some("Artist"), Some("Album"));
        assert_eq!(track_subtitle(&item), Some("Artist - Album".to_string()));
    }

    #[test]
    fn test_track_subtitle_artist_only() {
        let item = make_item(Some("Artist"), None);
        assert_eq!(track_subtitle(&item), Some("Artist".to_string()));
    }

    #[test]
    fn test_track_subtitle_album_only() {
        let item = make_item(None, Some("Album"));
        assert_eq!(track_subtitle(&item), Some("Album".to_string()));
    }

    #[test]
    fn test_track_subtitle_neither() {
        let item = make_item(None, None);
        assert_eq!(track_subtitle(&item), None);
    }

    #[test]
    fn test_track_subtitle_empty_strings_treated_as_none() {
        let mut item = make_item(None, None);
        item.grandparent_title = Some("".to_string());
        item.parent_title = Some("".to_string());
        assert_eq!(track_subtitle(&item), None);
    }

    #[test]
    fn test_format_time_delegates() {
        assert_eq!(format_time(65.0), simplex_core::ui_utils::format_time(65.0));
    }

    #[test]
    fn test_format_time_negative_clamped() {
        assert_eq!(format_time(-10.0), simplex_core::ui_utils::format_time(0.0));
    }
}
