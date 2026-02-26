//! Audio track language change detection.
//!
//! Monitors GStreamer's notify::current-audio signal and evaluates track preferences.
//! Extracts language information from GStreamer stream tags.

use gstreamer as gst;
use gstreamer::prelude::*;
use std::sync::{Arc, Mutex};
use simplex_core::media::{AudioTrack, MediaSession, TrackEvent, TrackPreference};

use super::pipeline::PlayerPipeline;

pub struct TrackMonitor {
    session: Arc<Mutex<MediaSession>>,
}

impl TrackMonitor {
    pub fn new(preference: TrackPreference) -> Self {
        let mut session = MediaSession::new();
        session.track_preference = preference;
        Self {
            session: Arc::new(Mutex::new(session)),
        }
    }

    pub fn session(&self) -> &Arc<Mutex<MediaSession>> {
        &self.session
    }

    /// Connect to the pipeline's notify::current-audio signal.
    pub fn connect(&self, pipeline: &Arc<Mutex<PlayerPipeline>>) {
        let session = self.session.clone();
        let pipe = pipeline.clone();

        let element = {
            let p = pipeline.lock().unwrap();
            p.element().clone()
        };

        element.connect_local("notify::current-audio", false, move |_args| {
            let pipe_guard = match pipe.try_lock() {
                Ok(g) => g,
                Err(_) => return None,
            };
            let current_idx = pipe_guard.current_audio_track();
            let language = get_audio_language(pipe_guard.element(), current_idx);

            let track = AudioTrack {
                index: current_idx,
                language,
                title: None,
                codec: None,
                channels: None,
            };

            let mut sess = session.lock().unwrap();
            sess.current_audio_track = Some(track.clone());

            if let Some(event) = sess.evaluate_audio_track_change(&track) {
                if let TrackEvent::PauseRequested { reason } = &event {
                    tracing::warn!("{}", reason);
                    pipe_guard.pause();
                }
            }

            None::<glib::Value>
        });
    }
}

/// Extract the language code from a GStreamer audio stream's tags.
fn get_audio_language(element: &gst::Element, index: i32) -> Option<String> {
    let tags: Option<gst::TagList> = element
        .emit_by_name("get-audio-tags", &[&index]);
    tags.and_then(|t| {
        t.get::<gst::tags::LanguageCode>()
            .map(|v| v.get().to_string())
    })
}
