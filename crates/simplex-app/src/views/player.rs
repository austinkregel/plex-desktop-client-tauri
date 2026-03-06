use gstreamer::MessageView;
use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Button, Label, Orientation};
use libadwaita::prelude::*;
use simplex_core::api::library::Marker;
use simplex_core::api::playback::TimelineState;
use simplex_core::media::TrackPreference;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::player::controls::PlayerControls;
use crate::player::logic::handle_settings_event;
use crate::player::pip::PipWindow;
use crate::player::pipeline::PlayerPipeline;
use crate::player::track_monitor::{MismatchWarning, TrackMonitor};
use crate::window::{AppState, SettingsEvent};

const TIMELINE_SYNC_INTERVAL: Duration = Duration::from_secs(5);
const WATCHED_COMPLETION_THRESHOLD: f64 = 0.90;
const SEEK_JUMP_SECONDS: f64 = 5.0;

struct PlaybackSyncContext {
    base_url: String,
    token: String,
    rating_key: String,
    metadata_key: String,
}

fn playback_sync_context(state: &Arc<Mutex<AppState>>) -> Option<PlaybackSyncContext> {
    let s = state.lock().unwrap();
    let token = s.token.clone()?;
    let base_url = s.base_url()?.to_string();
    let rating_key = s.playback_rating_key.clone()?;
    Some(PlaybackSyncContext {
        base_url,
        token,
        metadata_key: format!("/library/metadata/{rating_key}"),
        rating_key,
    })
}

use crate::player::logic;

fn secs_to_ms(value: f64) -> u64 {
    logic::secs_to_ms(value)
}

fn sync_timeline_with_plex(
    state: &Arc<Mutex<AppState>>,
    timeline_state: TimelineState,
    position_secs: Option<f64>,
    duration_secs: Option<f64>,
) {
    let Some(ctx) = playback_sync_context(state) else {
        return;
    };
    let time_ms = secs_to_ms(position_secs.unwrap_or(0.0));
    let duration_ms = duration_secs.map(secs_to_ms);

    crate::app::runtime().spawn(async move {
        if let Err(e) = simplex_core::api::playback::update_timeline(
            &ctx.base_url,
            &ctx.token,
            &ctx.rating_key,
            &ctx.metadata_key,
            time_ms,
            duration_ms,
            timeline_state,
        )
        .await
        {
            tracing::warn!("Timeline sync failed: {e}");
        }
    });
}

fn sync_completion_with_plex(
    state: &Arc<Mutex<AppState>>,
    position_secs: Option<f64>,
    duration_secs: Option<f64>,
    completion_scrobbled: &Arc<AtomicBool>,
) {
    let Some(ctx) = playback_sync_context(state) else {
        return;
    };

    let (Some(position), Some(duration)) = (position_secs, duration_secs) else {
        return;
    };
    if duration <= 0.0 {
        return;
    }

    let progress = (position / duration).clamp(0.0, 1.0);
    let was_scrobbled = completion_scrobbled.load(Ordering::Relaxed);

    enum CompletionAction {
        Scrobble,
        Unscrobble,
    }

    let action = if progress >= WATCHED_COMPLETION_THRESHOLD {
        completion_scrobbled.store(true, Ordering::Relaxed);
        Some(CompletionAction::Scrobble)
    } else if was_scrobbled {
        completion_scrobbled.store(false, Ordering::Relaxed);
        Some(CompletionAction::Unscrobble)
    } else {
        None
    };

    if let Some(action) = action {
        crate::app::runtime().spawn(async move {
            let result = match action {
                CompletionAction::Scrobble => {
                    simplex_core::api::playback::scrobble(
                        &ctx.base_url,
                        &ctx.token,
                        &ctx.rating_key,
                    )
                    .await
                }
                CompletionAction::Unscrobble => {
                    simplex_core::api::playback::unscrobble(
                        &ctx.base_url,
                        &ctx.token,
                        &ctx.rating_key,
                    )
                    .await
                }
            };
            if let Err(e) = result {
                tracing::warn!("Completion sync failed: {e}");
            }
        });
    }
}

fn sync_stop_state(
    state: &Arc<Mutex<AppState>>,
    pipeline: &Rc<RefCell<Option<Arc<Mutex<PlayerPipeline>>>>>,
    completion_scrobbled: &Arc<AtomicBool>,
) {
    let (position_secs, duration_secs) = if let Some(ref pipe) = *pipeline.borrow() {
        let p = pipe.lock().unwrap();
        (p.position(), p.duration())
    } else {
        (None, None)
    };

    sync_timeline_with_plex(state, TimelineState::Stopped, position_secs, duration_secs);
    sync_completion_with_plex(state, position_secs, duration_secs, completion_scrobbled);
}

fn stop_and_leave(
    state: &Arc<Mutex<AppState>>,
    pipeline: &Rc<RefCell<Option<Arc<Mutex<PlayerPipeline>>>>>,
    timer_id: &Rc<RefCell<Option<glib::SourceId>>>,
    last_uri: &Rc<RefCell<Option<String>>>,
    pip_window: &Rc<RefCell<Option<PipWindow>>>,
    completion_scrobbled: &Arc<AtomicBool>,
) {
    sync_stop_state(state, pipeline, completion_scrobbled);
    if let Some(ref pipe) = *pipeline.borrow() {
        pipe.lock().unwrap().stop();
    }
    if let Some(id) = timer_id.borrow_mut().take() {
        id.remove();
    }
    *last_uri.borrow_mut() = None;
    if let Some(ref pw) = *pip_window.borrow() {
        pw.hide();
    }
    crate::window::leave_player(state);
}

fn collapse_to_main(state: &Arc<Mutex<AppState>>, pip_window: &Rc<RefCell<Option<PipWindow>>>) {
    tracing::debug!("collapse_to_main: hiding PiP and collapsing player");
    if let Some(ref pw) = *pip_window.borrow() {
        pw.hide();
    }
    crate::window::collapse_player(state);
}

/// Fetch metadata for the currently playing item, update the controls, and store markers.
fn fetch_and_display_metadata(
    state: &Arc<Mutex<AppState>>,
    controls: &Rc<RefCell<Option<PlayerControls>>>,
    rating_key: &str,
    markers_out: &Rc<RefCell<Vec<Marker>>>,
    media_type_out: &Rc<RefCell<Option<String>>>,
) {
    let (token, base_url) = {
        let s = state.lock().unwrap();
        match s.token.clone().zip(s.base_url().map(String::from)) {
            Some(pair) => pair,
            None => return,
        }
    };

    let rk = rating_key.to_string();
    let (tx, rx) = async_channel::unbounded();

    crate::app::runtime().spawn(async move {
        let result = simplex_core::api::library::get_metadata(&base_url, &token, &rk).await;
        let _ = tx.send(result).await;
    });

    let ctrl_c = controls.clone();
    let markers_c = markers_out.clone();
    let media_type_c = media_type_out.clone();
    glib::spawn_future_local(async move {
        match rx.recv().await {
            Ok(Ok(item)) => {
                *media_type_c.borrow_mut() = item.media_type.clone();
                *markers_c.borrow_mut() = item.markers.clone();
                if !item.markers.is_empty() {
                    tracing::info!("Loaded {} marker(s) for {}", item.markers.len(), item.title);
                }
                if let Some(ref ctrl) = *ctrl_c.borrow() {
                    let show_name = item.grandparent_title.as_deref().unwrap_or(&item.title);
                    let episode_line = if item.grandparent_title.is_some() {
                        let mut ep = String::new();
                        if let (Some(si), Some(ei)) = (item.parent_index, item.index) {
                            ep.push_str(&format!("S{} \u{00b7} E{}", si, ei));
                        }
                        if item.grandparent_title.is_some() {
                            if !ep.is_empty() {
                                ep.push_str(" \u{2014} ");
                            }
                            ep.push_str(&item.title);
                        }
                        ep
                    } else {
                        String::new()
                    };
                    ctrl.set_metadata(show_name, &episode_line);
                }
            }
            Ok(Err(e)) => tracing::warn!("Failed to fetch metadata for player: {e:?}"),
            Err(e) => tracing::warn!("Channel error fetching metadata: {e:?}"),
        }
    });
}

/// Fetch adjacent episodes and enable/disable prev/next buttons accordingly.
fn fetch_adjacent_episodes(
    state: &Arc<Mutex<AppState>>,
    controls: &Rc<RefCell<Option<PlayerControls>>>,
    rating_key: &str,
    adjacent_cache: &Rc<RefCell<Option<AdjacentCache>>>,
) {
    let (token, base_url) = {
        let s = state.lock().unwrap();
        match s.token.clone().zip(s.base_url().map(String::from)) {
            Some(pair) => pair,
            None => return,
        }
    };

    let rk = rating_key.to_string();
    let (tx, rx) = async_channel::unbounded();

    let bu = base_url.clone();
    let tk = token.clone();
    crate::app::runtime().spawn(async move {
        let result = simplex_core::api::library::get_adjacent_episodes(&bu, &tk, &rk).await;
        let _ = tx.send(result).await;
    });

    let ctrl_c = controls.clone();
    let cache_c = adjacent_cache.clone();
    let base_url_c = base_url;
    let token_c = token;
    glib::spawn_future_local(async move {
        match rx.recv().await {
            Ok(Ok(adj)) => {
                tracing::info!(
                    "Adjacent episodes: prev={}, next={}",
                    adj.previous
                        .as_ref()
                        .map(|i| i.title.as_str())
                        .unwrap_or("none"),
                    adj.next
                        .as_ref()
                        .map(|i| i.title.as_str())
                        .unwrap_or("none"),
                );

                let prev_key = adj.previous.as_ref().map(|i| i.rating_key.clone());
                let next_key = adj.next.as_ref().map(|i| i.rating_key.clone());

                let prev_url = adj.previous.as_ref().and_then(|item| {
                    simplex_core::api::transcode::playback_url_for_item(
                        item,
                        &base_url_c,
                        &token_c,
                        "simplex-session",
                    )
                });
                let next_url = adj.next.as_ref().and_then(|item| {
                    simplex_core::api::transcode::playback_url_for_item(
                        item,
                        &base_url_c,
                        &token_c,
                        "simplex-session",
                    )
                });

                *cache_c.borrow_mut() = Some(AdjacentCache {
                    prev_url,
                    prev_title: adj.previous.as_ref().map(|i| i.title.clone()),
                    prev_rating_key: prev_key,
                    next_url,
                    next_title: adj.next.as_ref().map(|i| i.title.clone()),
                    next_rating_key: next_key,
                    next_thumb: adj.next.as_ref().and_then(|i| {
                        i.best_thumb().map(|t| {
                            simplex_core::api::library::thumb_url(&base_url_c, &token_c, t)
                        })
                    }),
                    next_parent_index: adj.next.as_ref().and_then(|i| i.parent_index),
                    next_index: adj.next.as_ref().and_then(|i| i.index),
                });

                if let Some(ref ctrl) = *ctrl_c.borrow() {
                    let cache = cache_c.borrow();
                    let c = cache.as_ref().unwrap();
                    ctrl.prev_button.set_sensitive(c.prev_url.is_some());
                    ctrl.next_button.set_sensitive(c.next_url.is_some());
                }
            }
            Ok(Err(e)) => tracing::warn!("Failed to fetch adjacent episodes: {e:?}"),
            Err(e) => tracing::warn!("Channel error fetching adjacent episodes: {e:?}"),
        }
    });
}

struct AdjacentCache {
    prev_url: Option<String>,
    prev_title: Option<String>,
    prev_rating_key: Option<String>,
    next_url: Option<String>,
    next_title: Option<String>,
    next_rating_key: Option<String>,
    next_thumb: Option<String>,
    next_parent_index: Option<u32>,
    next_index: Option<u32>,
}

/// Switch playback to an adjacent episode/track.
fn switch_episode(
    state: &Arc<Mutex<AppState>>,
    pipeline: &Rc<RefCell<Option<Arc<Mutex<PlayerPipeline>>>>>,
    controls: &Rc<RefCell<Option<PlayerControls>>>,
    _timer_id: &Rc<RefCell<Option<glib::SourceId>>>,
    last_uri: &Rc<RefCell<Option<String>>>,
    adjacent_cache: &Rc<RefCell<Option<AdjacentCache>>>,
    completion_scrobbled: &Arc<AtomicBool>,
    markers: &Rc<RefCell<Vec<Marker>>>,
    up_next_countdown: &Rc<Cell<Option<u8>>>,
    up_next_dismissed: &Rc<Cell<bool>>,
    playback_error: &Arc<AtomicBool>,
    media_type: &Rc<RefCell<Option<String>>>,
    uri: &str,
    title: &str,
    rating_key: &str,
) {
    sync_stop_state(state, pipeline, completion_scrobbled);
    playback_error.store(false, Ordering::Relaxed);
    *media_type.borrow_mut() = None;
    {
        let mut s = state.lock().unwrap();
        s.playback_uri = Some(uri.to_string());
        s.playback_title = Some(title.to_string());
        s.playback_rating_key = Some(rating_key.to_string());
    }

    *last_uri.borrow_mut() = Some(uri.to_string());

    markers.borrow_mut().clear();
    up_next_countdown.set(None);
    up_next_dismissed.set(false);

    if let Some(ref pipe) = *pipeline.borrow() {
        let mut p = pipe.lock().unwrap();
        p.stop();
        p.set_uri(uri);
        p.play();
    }

    if let Some(ref ctrl) = *controls.borrow() {
        ctrl.play_pause_button
            .set_icon_name("media-playback-pause-symbolic");
        ctrl.title_label.set_text(title);
        ctrl.prev_button.set_sensitive(false);
        ctrl.next_button.set_sensitive(false);
        ctrl.hide_skip_action();
        ctrl.hide_up_next();
    }

    *adjacent_cache.borrow_mut() = None;
    fetch_and_display_metadata(state, controls, rating_key, markers, media_type);
    fetch_adjacent_episodes(state, controls, rating_key, adjacent_cache);
}

pub fn build(state: Arc<Mutex<AppState>>) -> GtkBox {
    let container = GtkBox::new(Orientation::Vertical, 0);
    container.set_vexpand(true);
    container.set_hexpand(true);
    container.add_css_class("player-view");

    let pipeline: Rc<RefCell<Option<Arc<Mutex<PlayerPipeline>>>>> = Rc::new(RefCell::new(None));
    let controls: Rc<RefCell<Option<PlayerControls>>> = Rc::new(RefCell::new(None));
    let pip_window: Rc<RefCell<Option<PipWindow>>> = Rc::new(RefCell::new(None));
    let timer_id: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));
    let last_uri: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    let adjacent_cache: Rc<RefCell<Option<AdjacentCache>>> = Rc::new(RefCell::new(None));
    let completion_scrobbled = Arc::new(AtomicBool::new(false));
    let last_sync_at: Rc<RefCell<Option<Instant>>> = Rc::new(RefCell::new(None));
    let last_position_sample: Rc<RefCell<Option<(Instant, f64)>>> = Rc::new(RefCell::new(None));
    let last_playing_state: Rc<RefCell<Option<bool>>> = Rc::new(RefCell::new(None));
    let markers: Rc<RefCell<Vec<Marker>>> = Rc::new(RefCell::new(Vec::new()));
    let up_next_countdown: Rc<Cell<Option<u8>>> = Rc::new(Cell::new(None));
    let up_next_dismissed: Rc<Cell<bool>> = Rc::new(Cell::new(false));
    let up_next_tick: Rc<Cell<u8>> = Rc::new(Cell::new(0));
    let skip_end_secs: Rc<Cell<Option<f64>>> = Rc::new(Cell::new(None));
    let playback_error: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    let current_media_type: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));

    let state_c = state.clone();
    let pipeline_c = pipeline.clone();
    let controls_c = controls.clone();
    let pip_c = pip_window.clone();
    let timer_c = timer_id.clone();
    let last_uri_c = last_uri.clone();
    let container_c = container.clone();
    let adjacent_c = adjacent_cache.clone();
    let completion_scrobbled_c = completion_scrobbled.clone();
    let last_sync_at_c = last_sync_at.clone();
    let last_position_sample_c = last_position_sample.clone();
    let last_playing_state_c = last_playing_state.clone();
    let markers_c = markers.clone();
    let up_next_countdown_c = up_next_countdown.clone();
    let up_next_dismissed_c = up_next_dismissed.clone();
    let up_next_tick_c = up_next_tick.clone();
    let skip_end_secs_c = skip_end_secs.clone();

    container.connect_map(move |_| {
        let (uri, title, rating_key, offset) = {
            let s = state_c.lock().unwrap();
            (s.playback_uri.clone(), s.playback_title.clone(),
             s.playback_rating_key.clone(), s.playback_offset)
        };

        let uri = match uri {
            Some(u) => u,
            None => return,
        };

        if last_uri_c.borrow().as_deref() == Some(&uri) {
            return;
        }
        *last_uri_c.borrow_mut() = Some(uri.clone());

        if pipeline_c.borrow().is_none() {
            let pipe = match PlayerPipeline::new() {
                Ok(p) => Arc::new(Mutex::new(p)),
                Err(e) => {
                    tracing::error!("Failed to create player pipeline: {}", e);
                    while let Some(child) = container_c.first_child() {
                        container_c.remove(&child);
                    }
                    let err_box = GtkBox::new(Orientation::Vertical, 12);
                    err_box.set_valign(gtk4::Align::Center);
                    err_box.set_halign(gtk4::Align::Center);
                    err_box.set_vexpand(true);
                    let err_label = Label::new(Some("Cannot start playback"));
                    err_label.add_css_class("title-2");
                    err_box.append(&err_label);
                    let detail_label = Label::new(Some(&e));
                    detail_label.add_css_class("dim-label");
                    detail_label.set_wrap(true);
                    detail_label.set_max_width_chars(60);
                    err_box.append(&detail_label);
                    let back_btn = Button::with_label("Go Back");
                    back_btn.add_css_class("suggested-action");
                    back_btn.add_css_class("pill");
                    let state_err = state_c.clone();
                    back_btn.connect_clicked(move |_| {
                        crate::window::leave_player(&state_err);
                    });
                    err_box.append(&back_btn);
                    container_c.append(&err_box);
                    *last_uri_c.borrow_mut() = None;
                    return;
                }
            };

            let (eos_tx, eos_rx) = async_channel::bounded::<()>(1);
            {
                let state_bus = state_c.clone();
                let pipe_bus = pipe.clone();
                let completion_bus = completion_scrobbled_c.clone();
                let error_bus = playback_error.clone();
                let mut p = pipe.lock().unwrap();
                p.connect_bus(move |msg| {
                    match msg {
                        MessageView::Error(e) => {
                            tracing::error!("GStreamer playback error: {}", e.error());
                            if let Some(dbg_info) = e.debug() {
                                tracing::debug!("GStreamer debug: {:?}", dbg_info);
                            }
                            error_bus.store(true, Ordering::Relaxed);
                        }
                        MessageView::Eos(_) => {
                            if error_bus.load(Ordering::Relaxed) {
                                tracing::warn!(
                                    "End of stream after error — not auto-advancing"
                                );
                                return;
                            }
                            tracing::info!("End of stream");
                            let (pos, dur) = {
                                let p = pipe_bus.lock().unwrap();
                                (p.position(), p.duration())
                            };
                            sync_timeline_with_plex(&state_bus, TimelineState::Stopped, pos, dur);
                            sync_completion_with_plex(&state_bus, pos, dur, &completion_bus);
                            let _ = eos_tx.try_send(());
                        }
                        _ => {}
                    }
                });
            }

            // Auto-play next episode on EOS
            {
                let s_eos = state_c.clone();
                let pl_eos = pipeline_c.clone();
                let ctrl_eos = controls_c.clone();
                let ti_eos = timer_c.clone();
                let lu_eos = last_uri_c.clone();
                let adj_eos = adjacent_c.clone();
                let comp_eos = completion_scrobbled_c.clone();
                let mk_eos = markers_c.clone();
                let cd_eos = up_next_countdown_c.clone();
                let dis_eos = up_next_dismissed_c.clone();
                let err_eos = playback_error.clone();
                let mt_eos = current_media_type.clone();
                glib::spawn_future_local(async move {
                    while eos_rx.recv().await.is_ok() {
                        let info = {
                            let cache = adj_eos.borrow();
                            cache.as_ref().and_then(|c| {
                                c.next_url.as_ref().map(|url| {
                                    (
                                        url.clone(),
                                        c.next_title.clone().unwrap_or_default(),
                                        c.next_rating_key.clone().unwrap_or_default(),
                                    )
                                })
                            })
                        };
                        if let Some((url, title, rk)) = info {
                            tracing::info!("Auto-playing next: {}", title);
                            switch_episode(
                                &s_eos, &pl_eos, &ctrl_eos, &ti_eos, &lu_eos,
                                &adj_eos, &comp_eos, &mk_eos, &cd_eos, &dis_eos,
                                &err_eos, &mt_eos,
                                &url, &title, &rk,
                            );
                        }
                    }
                });
            }

            let ctrl = PlayerControls::new(&pipe);

            if let Some(ref t) = title {
                ctrl.title_label.set_text(t);
            }

            // Back button -> stop and leave
            {
                let s = state_c.clone();
                let pw = pip_c.clone();
                ctrl.back_button.connect_clicked(move |_| {
                    collapse_to_main(&s, &pw);
                });
            }

            // Stop button -> stop and leave
            {
                let s = state_c.clone();
                let pl = pipeline_c.clone();
                let ti = timer_c.clone();
                let lu = last_uri_c.clone();
                let pw = pip_c.clone();
                let completion = completion_scrobbled_c.clone();
                ctrl.stop_button.connect_clicked(move |_| {
                    stop_and_leave(&s, &pl, &ti, &lu, &pw, &completion);
                });
            }

            // Skip back 10s
            {
                let pipe_skip = pipe.clone();
                let state_skip = state_c.clone();
                ctrl.skip_back_button.connect_clicked(move |_| {
                    let p = pipe_skip.lock().unwrap();
                    if let Some(pos) = p.position() {
                        p.seek((pos - 10.0).max(0.0));
                        sync_timeline_with_plex(
                            &state_skip,
                            if p.is_playing() { TimelineState::Playing } else { TimelineState::Paused },
                            p.position(),
                            p.duration(),
                        );
                    }
                });
            }

            // Skip forward 30s
            {
                let pipe_skip = pipe.clone();
                let state_skip = state_c.clone();
                ctrl.skip_forward_button.connect_clicked(move |_| {
                    let p = pipe_skip.lock().unwrap();
                    if let Some(pos) = p.position() {
                        let dur = p.duration().unwrap_or(f64::MAX);
                        p.seek((pos + 30.0).min(dur));
                        sync_timeline_with_plex(
                            &state_skip,
                            if p.is_playing() { TimelineState::Playing } else { TimelineState::Paused },
                            p.position(),
                            p.duration(),
                        );
                    }
                });
            }

            // Previous episode
            {
                let s_prev = state_c.clone();
                let pl_prev = pipeline_c.clone();
                let ctrl_prev = controls_c.clone();
                let ti_prev = timer_c.clone();
                let lu_prev = last_uri_c.clone();
                let adj_prev = adjacent_c.clone();
                let completion_prev = completion_scrobbled_c.clone();
                let mk_prev = markers_c.clone();
                let cd_prev = up_next_countdown_c.clone();
                let dis_prev = up_next_dismissed_c.clone();
                let err_prev = playback_error.clone();
                let mt_prev = current_media_type.clone();
                ctrl.prev_button.connect_clicked(move |_| {
                    let info = {
                        let cache = adj_prev.borrow();
                        cache.as_ref().and_then(|c| {
                            c.prev_url.as_ref().map(|url| {
                                (url.clone(), c.prev_title.clone().unwrap_or_default(),
                                 c.prev_rating_key.clone().unwrap_or_default())
                            })
                        })
                    };
                    if let Some((url, title, rk)) = info {
                        switch_episode(
                            &s_prev, &pl_prev, &ctrl_prev, &ti_prev, &lu_prev,
                            &adj_prev, &completion_prev, &mk_prev, &cd_prev, &dis_prev,
                            &err_prev, &mt_prev,
                            &url, &title, &rk,
                        );
                    }
                });
            }

            // Next episode
            {
                let s_next = state_c.clone();
                let pl_next = pipeline_c.clone();
                let ctrl_next = controls_c.clone();
                let ti_next = timer_c.clone();
                let lu_next = last_uri_c.clone();
                let adj_next = adjacent_c.clone();
                let completion_next = completion_scrobbled_c.clone();
                let mk_next = markers_c.clone();
                let cd_next = up_next_countdown_c.clone();
                let dis_next = up_next_dismissed_c.clone();
                let err_next = playback_error.clone();
                let mt_next = current_media_type.clone();
                ctrl.next_button.connect_clicked(move |_| {
                    let info = {
                        let cache = adj_next.borrow();
                        cache.as_ref().and_then(|c| {
                            c.next_url.as_ref().map(|url| {
                                (url.clone(), c.next_title.clone().unwrap_or_default(),
                                 c.next_rating_key.clone().unwrap_or_default())
                            })
                        })
                    };
                    if let Some((url, title, rk)) = info {
                        switch_episode(
                            &s_next, &pl_next, &ctrl_next, &ti_next, &lu_next,
                            &adj_next, &completion_next, &mk_next, &cd_next, &dis_next,
                            &err_next, &mt_next,
                            &url, &title, &rk,
                        );
                    }
                });
            }

            // Fullscreen toggle
            {
                let state_fs = state_c.clone();
                ctrl.fullscreen_button.connect_clicked(move |btn| {
                    let window = {
                        state_fs.lock().unwrap().window.clone()
                    };
                    if let Some(w) = window {
                        if w.is_fullscreen() {
                            w.set_fullscreened(false);
                            btn.set_icon_name("view-fullscreen-symbolic");
                        } else {
                            w.set_fullscreened(true);
                            btn.set_icon_name("view-restore-symbolic");
                        }
                    }
                });
            }

            // PiP
            let pipe_for_pip = pipe.clone();
            let pip_toggle = pip_c.clone();
            let state_pip = state_c.clone();
            let pipeline_pip = pipeline_c.clone();
            let timer_pip = timer_c.clone();
            let last_uri_pip = last_uri_c.clone();
            let completion_pip = completion_scrobbled_c.clone();
            ctrl.pip_button.connect_clicked(move |_| {
                let mut pw = pip_toggle.borrow_mut();
                if let Some(ref existing) = *pw {
                    if existing.is_visible() {
                        existing.hide();
                        return;
                    }
                }
                let pip = PipWindow::new(&pipe_for_pip);

                let state_close = state_pip.clone();
                let pipe_close = pipeline_pip.clone();
                let timer_close = timer_pip.clone();
                let uri_close = last_uri_pip.clone();
                let completion_close = completion_pip.clone();
                pip.on_close(move || {
                    sync_stop_state(&state_close, &pipe_close, &completion_close);
                    if let Some(ref p) = *pipe_close.borrow() {
                        p.lock().unwrap().stop();
                    }
                    if let Some(id) = timer_close.borrow_mut().take() {
                        id.remove();
                    }
                    *uri_close.borrow_mut() = None;
                    let mut s = state_close.lock().unwrap();
                    s.playback_uri = None;
                    s.playback_rating_key = None;
                    s.playback_pipeline = None;
                });

                let state_return = state_pip.clone();
                pip.on_return(move || {
                    crate::window::return_to_player(&state_return);
                });

                pip.show();
                *pw = Some(pip);

                crate::window::restore_chrome(&state_pip);
            });

            // Skip Intro/Credits button
            {
                let pipe_skip = pipe.clone();
                let skip_end = skip_end_secs_c.clone();
                ctrl.skip_action_button.connect_clicked(move |_| {
                    if let Some(target) = skip_end.get() {
                        let p = pipe_skip.lock().unwrap();
                        p.seek(target);
                    }
                });
            }

            // Up Next: Play Now
            {
                let s_play = state_c.clone();
                let pl_play = pipeline_c.clone();
                let ctrl_play = controls_c.clone();
                let ti_play = timer_c.clone();
                let lu_play = last_uri_c.clone();
                let adj_play = adjacent_c.clone();
                let comp_play = completion_scrobbled_c.clone();
                let mk_play = markers_c.clone();
                let cd_play = up_next_countdown_c.clone();
                let dis_play = up_next_dismissed_c.clone();
                let err_play = playback_error.clone();
                let mt_play = current_media_type.clone();
                ctrl.up_next_play_button.connect_clicked(move |_| {
                    let info = {
                        let cache = adj_play.borrow();
                        cache.as_ref().and_then(|c| {
                            c.next_url.as_ref().map(|url| {
                                (url.clone(), c.next_title.clone().unwrap_or_default(),
                                 c.next_rating_key.clone().unwrap_or_default())
                            })
                        })
                    };
                    if let Some((url, title, rk)) = info {
                        switch_episode(
                            &s_play, &pl_play, &ctrl_play, &ti_play, &lu_play,
                            &adj_play, &comp_play, &mk_play, &cd_play, &dis_play,
                            &err_play, &mt_play,
                            &url, &title, &rk,
                        );
                    }
                });
            }

            // Up Next: Cancel
            {
                let ctrl_cancel = controls_c.clone();
                let cd_cancel = up_next_countdown_c.clone();
                let dis_cancel = up_next_dismissed_c.clone();
                ctrl.up_next_cancel_button.connect_clicked(move |_| {
                    cd_cancel.set(None);
                    dis_cancel.set(true);
                    if let Some(ref ctrl) = *ctrl_cancel.borrow() {
                        ctrl.hide_up_next();
                    }
                });
            }

            while let Some(child) = container_c.first_child() {
                container_c.remove(&child);
            }

            container_c.append(&ctrl.widget);

            let user_settings = simplex_core::config::load_user_settings();
            let preference = TrackPreference::from_user_settings(&user_settings);

            {
                let p = pipe.lock().unwrap();
                p.set_preferred_audio_languages(
                    user_settings.audio.preferred_languages.clone(),
                );
            }

            let (warn_tx, warn_rx) = async_channel::unbounded::<MismatchWarning>();
            let mut monitor = TrackMonitor::new(preference);
            monitor.set_warning_sender(warn_tx);
            monitor.connect(&pipe);

            // Subscribe to real-time settings changes from the Settings page.
            {
                let session_arc = monitor.session().clone();
                let pipe_settings = pipe.clone();
                let settings_rx = state_c.lock().unwrap().settings_event_rx.take();
                if let Some(rx) = settings_rx {
                    glib::spawn_future_local(async move {
                        while let Ok(event) = rx.recv().await {
                            let needs_apply = matches!(
                                &event,
                                SettingsEvent::AudioLanguagesChanged(_)
                            );
                            {
                                let p = pipe_settings.lock().unwrap();
                                let mut sess = session_arc.lock().unwrap();
                                handle_settings_event(&*p, &mut sess, event);
                            }
                            if needs_apply {
                                let p = pipe_settings.lock().unwrap();
                                p.apply_preferred_audio_language();
                            }
                        }
                    });
                }
            }

            // Listen for language-mismatch warnings and show an adw::AlertDialog.
            {
                let state_warn = state_c.clone();
                let pipe_warn = pipe.clone();
                let ctrl_warn = controls_c.clone();
                glib::spawn_future_local(async move {
                    while let Ok(warning) = warn_rx.recv().await {
                        let window = {
                            state_warn.lock().unwrap().window.clone()
                        };
                        let Some(win) = window else { continue };

                        let dialog = libadwaita::AlertDialog::builder()
                            .heading("Audio Language Changed")
                            .body(format!(
                                "The audio switched to {}. Your preferred language ({}) is not available.",
                                warning.language, warning.preferred,
                            ))
                            .build();
                        dialog.add_response("continue", "Continue Anyway");
                        dialog.add_response("select", "Select Audio Track");
                        dialog.set_default_response(Some("continue"));
                        dialog.set_close_response("continue");

                        let pipe_d = pipe_warn.clone();
                        let ctrl_d = ctrl_warn.clone();
                        dialog.connect_response(None, move |_dlg, response| {
                            match response {
                                "continue" => {
                                    let p = pipe_d.lock().unwrap();
                                    p.play();
                                }
                                "select" => {
                                    let p = pipe_d.lock().unwrap();
                                    p.play();
                                    drop(p);
                                    if let Some(ref ctrl) = *ctrl_d.borrow() {
                                        ctrl.quick_settings_popover.popup();
                                    }
                                }
                                _ => {}
                            }
                        });
                        dialog.present(Some(&win));
                    }
                });
            }

            *controls_c.borrow_mut() = Some(ctrl);
            *pipeline_c.borrow_mut() = Some(pipe);
            if let Some(ref active_pipe) = *pipeline_c.borrow() {
                state_c.lock().unwrap().playback_pipeline = Some(active_pipe.clone());
            }
        }

        if let Some(ref pipe) = *pipeline_c.borrow() {
            {
                let mut p = pipe.lock().unwrap();
                p.stop();
                p.set_uri(&uri);
                p.play();
            }
            completion_scrobbled_c.store(false, Ordering::Relaxed);
            *last_sync_at_c.borrow_mut() = None;
            *last_position_sample_c.borrow_mut() = None;
            *last_playing_state_c.borrow_mut() = None;
            markers_c.borrow_mut().clear();
            up_next_countdown_c.set(None);
            up_next_dismissed_c.set(false);
            up_next_tick_c.set(0);
            skip_end_secs_c.set(None);
            playback_error.store(false, Ordering::Relaxed);
            *current_media_type.borrow_mut() = None;

            // Resume from offset after a brief delay so the pipeline reaches PLAYING
            if let Some(seek_pos) = offset {
                if seek_pos > 0.0 {
                    let pipe_seek = pipe.clone();
                    glib::timeout_add_local_once(
                        std::time::Duration::from_millis(300),
                        move || {
                            let p = pipe_seek.lock().unwrap();
                            p.seek(seek_pos);
                            tracing::info!("Resumed playback at {:.1}s", seek_pos);
                        },
                    );
                }
            }

            if let Some(ref ctrl) = *controls_c.borrow() {
                ctrl.play_pause_button.set_icon_name("media-playback-pause-symbolic");
                ctrl.prev_button.set_sensitive(false);
                ctrl.next_button.set_sensitive(false);
                if let Some(ref t) = title {
                    ctrl.title_label.set_text(t);
                }
            }

            if let Some(id) = timer_c.borrow_mut().take() {
                id.remove();
            }
            let pipe_timer = pipe.clone();
            let ctrl_timer = controls_c.clone();
            let state_timer = state_c.clone();
            let last_sync_timer = last_sync_at_c.clone();
            let last_sample_timer = last_position_sample_c.clone();
            let last_playing_timer = last_playing_state_c.clone();
            let markers_timer = markers_c.clone();
            let skip_end_timer = skip_end_secs_c.clone();
            let adj_timer = adjacent_c.clone();
            let countdown_timer = up_next_countdown_c.clone();
            let dismissed_timer = up_next_dismissed_c.clone();
            let tick_timer = up_next_tick_c.clone();
            let pl_auto = pipeline_c.clone();
            let ti_auto = timer_c.clone();
            let lu_auto = last_uri_c.clone();
            let comp_auto = completion_scrobbled_c.clone();
            let mk_auto = markers_c.clone();
            let cd_auto = up_next_countdown_c.clone();
            let dis_auto = up_next_dismissed_c.clone();
            let err_auto = playback_error.clone();
            let mt_auto = current_media_type.clone();
            let mt_timer = current_media_type.clone();
            let id = glib::timeout_add_local(std::time::Duration::from_millis(500), move || {
                let mut auto_play_info: Option<(String, String, String)> = None;

                if let Some(ref ctrl) = *ctrl_timer.borrow() {
                    let p = pipe_timer.lock().unwrap();
                    ctrl.update_position(&p);
                    let now = Instant::now();
                    let pos = p.position();
                    let dur = p.duration();
                    let is_playing = p.is_playing();

                    if let Some(prev) = *last_playing_timer.borrow() {
                        if prev != is_playing {
                            sync_timeline_with_plex(
                                &state_timer,
                                if is_playing { TimelineState::Playing } else { TimelineState::Paused },
                                pos,
                                dur,
                            );
                            *last_sync_timer.borrow_mut() = Some(now);
                        }
                    }
                    *last_playing_timer.borrow_mut() = Some(is_playing);

                    if let Some(position) = pos {
                        if let Some((prev_at, prev_pos)) = *last_sample_timer.borrow() {
                            let elapsed = now.duration_since(prev_at).as_secs_f64();
                            let expected = if is_playing {
                                prev_pos + elapsed
                            } else {
                                prev_pos
                            };
                            if (position - expected).abs() > SEEK_JUMP_SECONDS {
                                sync_timeline_with_plex(
                                    &state_timer,
                                    if is_playing { TimelineState::Playing } else { TimelineState::Paused },
                                    Some(position),
                                    dur,
                                );
                                *last_sync_timer.borrow_mut() = Some(now);
                            }
                        }
                        *last_sample_timer.borrow_mut() = Some((now, position));

                        // --- Marker-driven skip button and Up Next ---
                        let position_ms = (position * 1000.0) as u64;
                        let mk = markers_timer.borrow();
                        let mut in_intro = false;
                        let mut in_credits = false;
                        let mut skip_target: Option<f64> = None;

                        for m in mk.iter() {
                            let (Some(start), Some(end)) = (m.start_time_offset, m.end_time_offset) else {
                                continue;
                            };
                            if position_ms >= start && position_ms < end {
                                match m.marker_type.as_deref() {
                                    Some("intro") => {
                                        in_intro = true;
                                        skip_target = Some(end as f64 / 1000.0);
                                    }
                                    Some("credits") => {
                                        in_credits = true;
                                        skip_target = Some(end as f64 / 1000.0);
                                    }
                                    _ => {}
                                }
                            }
                        }
                        drop(mk);

                        if in_intro {
                            skip_end_timer.set(skip_target);
                            ctrl.show_skip_action("Skip Intro");
                        } else if in_credits {
                            skip_end_timer.set(skip_target);
                            ctrl.show_skip_action("Skip Credits");
                        } else {
                            skip_end_timer.set(None);
                            ctrl.hide_skip_action();
                        }

                        // Up Next: show when credits start (or near end for episodes).
                        // Music tracks auto-advance only on real EOS, not a countdown.
                        let has_next = {
                            let cache = adj_timer.borrow();
                            cache.as_ref().map_or(false, |c| c.next_url.is_some())
                        };
                        let is_music = mt_timer.borrow().as_deref() == Some("track");

                        if has_next && !dismissed_timer.get() && !is_music {
                            let should_show_up_next = in_credits
                                || dur.map_or(false, |d| d > 0.0 && position >= d - 30.0);

                            if should_show_up_next && countdown_timer.get().is_none() {
                                countdown_timer.set(Some(5));
                                tick_timer.set(0);
                                let cache = adj_timer.borrow();
                                if let Some(ref c) = *cache {
                                    let title = c.next_title.as_deref().unwrap_or("Next Episode");
                                    let subtitle = match (c.next_parent_index, c.next_index) {
                                        (Some(si), Some(ei)) => format!("S{} \u{00b7} E{}", si, ei),
                                        _ => String::new(),
                                    };
                                    ctrl.show_up_next(title, &subtitle);
                                    ctrl.set_up_next_countdown(5);
                                    if let Some(ref thumb_url) = c.next_thumb {
                                        ctrl.load_up_next_thumb(thumb_url);
                                    }
                                }
                            }

                            if let Some(secs) = countdown_timer.get() {
                                let t = tick_timer.get() + 1;
                                tick_timer.set(t);
                                if t >= 2 {
                                    tick_timer.set(0);
                                    let new_secs = secs.saturating_sub(1);
                                    if new_secs == 0 {
                                        countdown_timer.set(None);
                                        auto_play_info = {
                                            let cache = adj_timer.borrow();
                                            cache.as_ref().and_then(|c| {
                                                c.next_url.as_ref().map(|url| {
                                                    (url.clone(), c.next_title.clone().unwrap_or_default(),
                                                     c.next_rating_key.clone().unwrap_or_default())
                                                })
                                            })
                                        };
                                    } else {
                                        countdown_timer.set(Some(new_secs));
                                        ctrl.set_up_next_countdown(new_secs);
                                    }
                                }
                            }
                        }
                    }

                    if is_playing {
                        let should_sync = match *last_sync_timer.borrow() {
                            Some(last) => now.duration_since(last) >= TIMELINE_SYNC_INTERVAL,
                            None => true,
                        };
                        if should_sync {
                            sync_timeline_with_plex(&state_timer, TimelineState::Playing, pos, dur);
                            *last_sync_timer.borrow_mut() = Some(now);
                        }
                    }
                }

                if let Some((url, title, rk)) = auto_play_info {
                    tracing::info!("Countdown finished, auto-playing: {}", title);
                    switch_episode(
                        &state_timer, &pl_auto, &ctrl_timer, &ti_auto,
                        &lu_auto, &adj_timer, &comp_auto,
                        &mk_auto, &cd_auto, &dis_auto,
                        &err_auto, &mt_auto,
                        &url, &title, &rk,
                    );
                }

                glib::ControlFlow::Continue
            });
            *timer_c.borrow_mut() = Some(id);
        }

        // Fetch metadata (with markers) and adjacent episodes
        if let Some(ref rk) = rating_key {
            fetch_and_display_metadata(&state_c, &controls_c, rk, &markers_c, &current_media_type);
            fetch_adjacent_episodes(&state_c, &controls_c, rk, &adjacent_c);
        }
    });

    // Keyboard shortcuts
    let key_controller = gtk4::EventControllerKey::new();
    let pipeline_key = pipeline.clone();
    let controls_key = controls.clone();
    let state_key = state.clone();
    let pip_key = pip_window.clone();
    key_controller.connect_key_pressed(move |_, keyval, _, _| {
        use gtk4::gdk::Key;
        match keyval {
            k if k == Key::space => {
                if let Some(ref pipe) = *pipeline_key.borrow() {
                    let p = pipe.lock().unwrap();
                    p.toggle_play_pause();
                    sync_timeline_with_plex(
                        &state_key,
                        if p.is_playing() {
                            TimelineState::Playing
                        } else {
                            TimelineState::Paused
                        },
                        p.position(),
                        p.duration(),
                    );
                    if let Some(ref ctrl) = *controls_key.borrow() {
                        if p.is_playing() {
                            ctrl.play_pause_button
                                .set_icon_name("media-playback-pause-symbolic");
                        } else {
                            ctrl.play_pause_button
                                .set_icon_name("media-playback-start-symbolic");
                        }
                    }
                }
                glib::Propagation::Stop
            }
            k if k == Key::Escape => {
                collapse_to_main(&state_key, &pip_key);
                glib::Propagation::Stop
            }
            k if k == Key::F11 => {
                let window = { state_key.lock().unwrap().window.clone() };
                if let Some(w) = window {
                    if w.is_fullscreen() {
                        w.set_fullscreened(false);
                    } else {
                        w.set_fullscreened(true);
                    }
                }
                glib::Propagation::Stop
            }
            k if k == Key::Left => {
                if let Some(ref pipe) = *pipeline_key.borrow() {
                    let p = pipe.lock().unwrap();
                    if let Some(pos) = p.position() {
                        p.seek((pos - 10.0).max(0.0));
                        sync_timeline_with_plex(
                            &state_key,
                            if p.is_playing() {
                                TimelineState::Playing
                            } else {
                                TimelineState::Paused
                            },
                            p.position(),
                            p.duration(),
                        );
                    }
                }
                glib::Propagation::Stop
            }
            k if k == Key::Right => {
                if let Some(ref pipe) = *pipeline_key.borrow() {
                    let p = pipe.lock().unwrap();
                    if let Some(pos) = p.position() {
                        let dur = p.duration().unwrap_or(f64::MAX);
                        p.seek((pos + 10.0).min(dur));
                        sync_timeline_with_plex(
                            &state_key,
                            if p.is_playing() {
                                TimelineState::Playing
                            } else {
                                TimelineState::Paused
                            },
                            p.position(),
                            p.duration(),
                        );
                    }
                }
                glib::Propagation::Stop
            }
            k if k == Key::Up => {
                if let Some(ref pipe) = *pipeline_key.borrow() {
                    let p = pipe.lock().unwrap();
                    let vol = (p.volume() + 0.05).min(1.5);
                    p.set_volume(vol);
                }
                glib::Propagation::Stop
            }
            k if k == Key::Down => {
                if let Some(ref pipe) = *pipeline_key.borrow() {
                    let p = pipe.lock().unwrap();
                    let vol = (p.volume() - 0.05).max(0.0);
                    p.set_volume(vol);
                }
                glib::Propagation::Stop
            }
            _ => glib::Propagation::Proceed,
        }
    });
    container.add_controller(key_controller);
    container.set_focusable(true);

    container
}
