use gstreamer::MessageView;
use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Button, Label, Orientation};
use simplex_core::api::playback::TimelineState;
use simplex_core::media::TrackPreference;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::player::controls::PlayerControls;
use crate::player::pip::PipWindow;
use crate::player::pipeline::PlayerPipeline;
use crate::player::track_monitor::TrackMonitor;
use crate::window::AppState;

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

fn secs_to_ms(value: f64) -> u64 {
    (value.max(0.0) * 1000.0) as u64
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
                    simplex_core::api::playback::scrobble(&ctx.base_url, &ctx.token, &ctx.rating_key).await
                }
                CompletionAction::Unscrobble => {
                    simplex_core::api::playback::unscrobble(&ctx.base_url, &ctx.token, &ctx.rating_key).await
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

/// Fetch metadata for the currently playing item and update the controls.
fn fetch_and_display_metadata(
    state: &Arc<Mutex<AppState>>,
    controls: &Rc<RefCell<Option<PlayerControls>>>,
    rating_key: &str,
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
    glib::spawn_future_local(async move {
        match rx.recv().await {
            Ok(Ok(item)) => {
                if let Some(ref ctrl) = *ctrl_c.borrow() {
                    let show_name = item.grandparent_title.as_deref()
                        .unwrap_or(&item.title);
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
                    adj.previous.as_ref().map(|i| i.title.as_str()).unwrap_or("none"),
                    adj.next.as_ref().map(|i| i.title.as_str()).unwrap_or("none"),
                );

                let prev_key = adj.previous.as_ref().map(|i| i.rating_key.clone());
                let next_key = adj.next.as_ref().map(|i| i.rating_key.clone());

                let prev_url = adj.previous.as_ref().and_then(|item| {
                    simplex_core::api::transcode::playback_url_for_item(
                        item, &base_url_c, &token_c, "simplex-session",
                    )
                });
                let next_url = adj.next.as_ref().and_then(|item| {
                    simplex_core::api::transcode::playback_url_for_item(
                        item, &base_url_c, &token_c, "simplex-session",
                    )
                });

                *cache_c.borrow_mut() = Some(AdjacentCache {
                    prev_url,
                    prev_title: adj.previous.as_ref().map(|i| i.title.clone()),
                    prev_rating_key: prev_key,
                    next_url,
                    next_title: adj.next.as_ref().map(|i| i.title.clone()),
                    next_rating_key: next_key,
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
}

/// Switch playback to an adjacent episode.
fn switch_episode(
    state: &Arc<Mutex<AppState>>,
    pipeline: &Rc<RefCell<Option<Arc<Mutex<PlayerPipeline>>>>>,
    controls: &Rc<RefCell<Option<PlayerControls>>>,
    _timer_id: &Rc<RefCell<Option<glib::SourceId>>>,
    last_uri: &Rc<RefCell<Option<String>>>,
    adjacent_cache: &Rc<RefCell<Option<AdjacentCache>>>,
    completion_scrobbled: &Arc<AtomicBool>,
    uri: &str,
    title: &str,
    rating_key: &str,
) {
    sync_stop_state(state, pipeline, completion_scrobbled);
    {
        let mut s = state.lock().unwrap();
        s.playback_uri = Some(uri.to_string());
        s.playback_title = Some(title.to_string());
        s.playback_rating_key = Some(rating_key.to_string());
    }

    *last_uri.borrow_mut() = Some(uri.to_string());

    if let Some(ref pipe) = *pipeline.borrow() {
        let mut p = pipe.lock().unwrap();
        p.stop();
        p.set_uri(uri);
        p.play();
    }

    if let Some(ref ctrl) = *controls.borrow() {
        ctrl.play_pause_button.set_icon_name("media-playback-pause-symbolic");
        ctrl.title_label.set_text(title);
        ctrl.prev_button.set_sensitive(false);
        ctrl.next_button.set_sensitive(false);
    }

    // Reset adjacent cache and re-fetch
    *adjacent_cache.borrow_mut() = None;
    fetch_and_display_metadata(state, controls, rating_key);
    fetch_adjacent_episodes(state, controls, rating_key, adjacent_cache);
}

pub fn build(state: Arc<Mutex<AppState>>) -> GtkBox {
    let container = GtkBox::new(Orientation::Vertical, 0);
    container.set_vexpand(true);
    container.set_hexpand(true);
    container.add_css_class("player-view");

    let pipeline: Rc<RefCell<Option<Arc<Mutex<PlayerPipeline>>>>> =
        Rc::new(RefCell::new(None));
    let controls: Rc<RefCell<Option<PlayerControls>>> = Rc::new(RefCell::new(None));
    let pip_window: Rc<RefCell<Option<PipWindow>>> = Rc::new(RefCell::new(None));
    let timer_id: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));
    let last_uri: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    let adjacent_cache: Rc<RefCell<Option<AdjacentCache>>> = Rc::new(RefCell::new(None));
    let completion_scrobbled = Arc::new(AtomicBool::new(false));
    let last_sync_at: Rc<RefCell<Option<Instant>>> = Rc::new(RefCell::new(None));
    let last_position_sample: Rc<RefCell<Option<(Instant, f64)>>> = Rc::new(RefCell::new(None));
    let last_playing_state: Rc<RefCell<Option<bool>>> = Rc::new(RefCell::new(None));

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

            {
                let state_bus = state_c.clone();
                let pipe_bus = pipe.clone();
                let completion_bus = completion_scrobbled_c.clone();
                let mut p = pipe.lock().unwrap();
                p.connect_bus(move |msg| {
                    match msg {
                        MessageView::Error(e) => {
                            tracing::error!("GStreamer error: {}", e.error());
                        }
                        MessageView::Eos(_) => {
                            tracing::info!("End of stream");
                            let (pos, dur) = {
                                let p = pipe_bus.lock().unwrap();
                                (p.position(), p.duration())
                            };
                            sync_timeline_with_plex(&state_bus, TimelineState::Stopped, pos, dur);
                            sync_completion_with_plex(&state_bus, pos, dur, &completion_bus);
                        }
                        _ => {}
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
                let pl = pipeline_c.clone();
                let ti = timer_c.clone();
                let lu = last_uri_c.clone();
                let pw = pip_c.clone();
                let completion = completion_scrobbled_c.clone();
                ctrl.back_button.connect_clicked(move |_| {
                    stop_and_leave(&s, &pl, &ti, &lu, &pw, &completion);
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
                            &adj_prev, &completion_prev, &url, &title, &rk,
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
                            &adj_next, &completion_next, &url, &title, &rk,
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
                });

                let state_return = state_pip.clone();
                pip.on_return(move || {
                    crate::window::return_to_player(&state_return);
                });

                pip.show();
                *pw = Some(pip);

                crate::window::restore_chrome(&state_pip);
            });

            while let Some(child) = container_c.first_child() {
                container_c.remove(&child);
            }

            container_c.append(&ctrl.widget);

            let monitor = TrackMonitor::new(TrackPreference::default());
            monitor.connect(&pipe);

            *controls_c.borrow_mut() = Some(ctrl);
            *pipeline_c.borrow_mut() = Some(pipe);
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
            let id = glib::timeout_add_local(std::time::Duration::from_millis(500), move || {
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
                glib::ControlFlow::Continue
            });
            *timer_c.borrow_mut() = Some(id);
        }

        // Fetch metadata and adjacent episodes
        if let Some(ref rk) = rating_key {
            fetch_and_display_metadata(&state_c, &controls_c, rk);
            fetch_adjacent_episodes(&state_c, &controls_c, rk, &adjacent_c);
        }
    });

    // Keyboard shortcuts
    let key_controller = gtk4::EventControllerKey::new();
    let pipeline_key = pipeline.clone();
    let controls_key = controls.clone();
    let state_key = state.clone();
    let timer_key = timer_id.clone();
    let last_uri_key = last_uri.clone();
    let pip_key = pip_window.clone();
    let completion_key = completion_scrobbled.clone();
    key_controller.connect_key_pressed(move |_, keyval, _, _| {
        use gtk4::gdk::Key;
        match keyval {
            k if k == Key::space => {
                if let Some(ref pipe) = *pipeline_key.borrow() {
                    let p = pipe.lock().unwrap();
                    p.toggle_play_pause();
                    sync_timeline_with_plex(
                        &state_key,
                        if p.is_playing() { TimelineState::Playing } else { TimelineState::Paused },
                        p.position(),
                        p.duration(),
                    );
                    if let Some(ref ctrl) = *controls_key.borrow() {
                        if p.is_playing() {
                            ctrl.play_pause_button.set_icon_name("media-playback-pause-symbolic");
                        } else {
                            ctrl.play_pause_button.set_icon_name("media-playback-start-symbolic");
                        }
                    }
                }
                glib::Propagation::Stop
            }
            k if k == Key::Escape => {
                stop_and_leave(
                    &state_key,
                    &pipeline_key,
                    &timer_key,
                    &last_uri_key,
                    &pip_key,
                    &completion_key,
                );
                glib::Propagation::Stop
            }
            k if k == Key::F11 => {
                let window = {
                    state_key.lock().unwrap().window.clone()
                };
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
                            if p.is_playing() { TimelineState::Playing } else { TimelineState::Paused },
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
                            if p.is_playing() { TimelineState::Playing } else { TimelineState::Paused },
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
