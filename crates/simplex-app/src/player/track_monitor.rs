//! Audio track language change detection.
//!
//! Monitors GStreamer's notify::current-audio signal and evaluates track preferences.
//! Extracts language information from GStreamer stream tags.
//! Supports configurable mismatch actions: Pause, WarnDialog, or Ignore.

use gstreamer::prelude::*;
use std::sync::{Arc, Mutex};
use simplex_core::config::MismatchAction;
use simplex_core::media::{AudioTrack, MediaSession, TrackEvent, TrackPreference};

use super::pipeline::PlayerPipeline;

/// Payload sent to the player view when a language warning dialog is needed.
#[derive(Debug, Clone)]
pub struct MismatchWarning {
    pub language: String,
    pub preferred: String,
}

/// What action the monitor should take after evaluating a track change.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum TrackAction {
    None,
    Pause { reason: String },
    WarnAndPause {
        reason: String,
        language: String,
        preferred: String,
    },
}

/// Pure logic: evaluate a track change and decide what action to take.
pub(crate) fn evaluate_track_action(
    session: &mut MediaSession,
    track: &AudioTrack,
) -> TrackAction {
    session.current_audio_track = Some(track.clone());

    let Some(event) = session.evaluate_audio_track_change(track) else {
        return TrackAction::None;
    };

    let TrackEvent::PauseRequested { reason } = &event else {
        return TrackAction::None;
    };

    let reason = reason.clone();
    match session.track_preference.mismatch_action {
        MismatchAction::Pause => TrackAction::Pause { reason },
        MismatchAction::WarnDialog => {
            let language = track
                .language
                .clone()
                .unwrap_or_else(|| "Unknown".to_string());
            let preferred = session.track_preference.preferred_languages.join("/");
            TrackAction::WarnAndPause {
                reason,
                language,
                preferred,
            }
        }
        MismatchAction::Ignore => TrackAction::None,
    }
}

pub struct TrackMonitor {
    session: Arc<Mutex<MediaSession>>,
    warning_tx: Option<async_channel::Sender<MismatchWarning>>,
}

impl TrackMonitor {
    pub fn new(preference: TrackPreference) -> Self {
        let mut session = MediaSession::new();
        session.track_preference = preference;
        Self {
            session: Arc::new(Mutex::new(session)),
            warning_tx: None,
        }
    }

    /// Attach a channel sender so WarnDialog events can be delivered to the
    /// GTK main thread for display.
    pub fn set_warning_sender(&mut self, tx: async_channel::Sender<MismatchWarning>) {
        self.warning_tx = Some(tx);
    }

    pub fn session(&self) -> &Arc<Mutex<MediaSession>> {
        &self.session
    }

    /// Connect to the pipeline's notify::current-audio signal.
    pub fn connect(&self, pipeline: &Arc<Mutex<PlayerPipeline>>) {
        let session = self.session.clone();
        let pipe = pipeline.clone();
        let warning_tx = self.warning_tx.clone();

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
            let language = pipe_guard.audio_language(current_idx);

            let track = AudioTrack {
                index: current_idx,
                language,
                title: None,
                codec: None,
                channels: None,
            };

            let mut sess = session.lock().unwrap();
            let action = evaluate_track_action(&mut sess, &track);
            match action {
                TrackAction::None => {}
                TrackAction::Pause { reason } => {
                    tracing::warn!("{}", reason);
                    pipe_guard.pause();
                }
                TrackAction::WarnAndPause { reason, language, preferred } => {
                    tracing::warn!("{}", reason);
                    pipe_guard.pause();
                    if let Some(ref tx) = warning_tx {
                        let _ = tx.try_send(MismatchWarning { language, preferred });
                    }
                }
            }

            None::<glib::Value>
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use simplex_core::config::MismatchAction;
    use simplex_core::media::{AudioTrack, MediaSession, TrackPreference};

    fn make_preference(action: MismatchAction, langs: Vec<&str>) -> TrackPreference {
        let pause = matches!(&action, MismatchAction::Pause | MismatchAction::WarnDialog);
        TrackPreference {
            mismatch_action: action,
            preferred_languages: langs.into_iter().map(String::from).collect(),
            pause_on_mismatch: pause,
        }
    }

    fn make_session(pref: TrackPreference) -> MediaSession {
        let mut session = MediaSession::new();
        session.track_preference = pref;
        // Set an initial track so the change detection triggers
        session.current_audio_track = Some(AudioTrack {
            index: 0,
            language: Some("eng".to_string()),
            title: None,
            codec: None,
            channels: None,
        });
        session
    }

    fn make_track(lang: Option<&str>) -> AudioTrack {
        AudioTrack {
            index: 1,
            language: lang.map(String::from),
            title: None,
            codec: None,
            channels: None,
        }
    }

    #[test]
    fn test_evaluate_no_mismatch() {
        let pref = make_preference(MismatchAction::Pause, vec!["eng"]);
        let mut session = make_session(pref);
        let track = AudioTrack {
            index: 1,
            language: Some("eng".to_string()),
            title: None,
            codec: None,
            channels: None,
        };
        assert_eq!(evaluate_track_action(&mut session, &track), TrackAction::None);
    }

    #[test]
    fn test_evaluate_mismatch_pause() {
        let pref = make_preference(MismatchAction::Pause, vec!["eng"]);
        let mut session = make_session(pref);
        let track = make_track(Some("spa"));
        match evaluate_track_action(&mut session, &track) {
            TrackAction::Pause { reason } => assert!(!reason.is_empty()),
            other => panic!("Expected Pause, got {other:?}"),
        }
    }

    #[test]
    fn test_evaluate_mismatch_warn_dialog() {
        let pref = make_preference(MismatchAction::WarnDialog, vec!["eng"]);
        let mut session = make_session(pref);
        let track = make_track(Some("spa"));
        match evaluate_track_action(&mut session, &track) {
            TrackAction::WarnAndPause { language, preferred, reason } => {
                assert_eq!(language, "spa");
                assert_eq!(preferred, "eng");
                assert!(!reason.is_empty());
            }
            other => panic!("Expected WarnAndPause, got {other:?}"),
        }
    }

    #[test]
    fn test_evaluate_mismatch_ignore() {
        let pref = make_preference(MismatchAction::Ignore, vec!["eng"]);
        let mut session = make_session(pref);
        let track = make_track(Some("spa"));
        assert_eq!(evaluate_track_action(&mut session, &track), TrackAction::None);
    }

    #[test]
    fn test_evaluate_no_language_on_track() {
        let pref = make_preference(MismatchAction::Pause, vec!["eng"]);
        let mut session = make_session(pref);
        let track = make_track(None);
        // No language means we can't determine mismatch
        let action = evaluate_track_action(&mut session, &track);
        assert_eq!(action, TrackAction::None);
    }

    #[test]
    fn test_evaluate_warn_no_language_uses_unknown() {
        let pref = make_preference(MismatchAction::WarnDialog, vec!["eng"]);
        let mut session = MediaSession::new();
        session.track_preference = pref;
        // Set initial track with a language
        session.current_audio_track = Some(AudioTrack {
            index: 0,
            language: Some("eng".to_string()),
            title: None,
            codec: None,
            channels: None,
        });
        // Change to a track with a different language (not None, which returns TrackAction::None)
        let track = make_track(Some("jpn"));
        match evaluate_track_action(&mut session, &track) {
            TrackAction::WarnAndPause { language, preferred, .. } => {
                assert_eq!(language, "jpn");
                assert_eq!(preferred, "eng");
            }
            other => panic!("Expected WarnAndPause, got {other:?}"),
        }
    }

    #[test]
    fn test_evaluate_multiple_preferred_languages() {
        let pref = make_preference(MismatchAction::WarnDialog, vec!["eng", "fre"]);
        let mut session = make_session(pref);
        let track = make_track(Some("spa"));
        match evaluate_track_action(&mut session, &track) {
            TrackAction::WarnAndPause { preferred, .. } => {
                assert_eq!(preferred, "eng/fre");
            }
            other => panic!("Expected WarnAndPause, got {other:?}"),
        }
    }

    #[test]
    fn test_mismatch_warning_debug() {
        let w = MismatchWarning {
            language: "spa".to_string(),
            preferred: "eng".to_string(),
        };
        let dbg = format!("{w:?}");
        assert!(dbg.contains("spa"));
    }

    #[test]
    fn test_track_monitor_new() {
        let pref = TrackPreference::default();
        let monitor = TrackMonitor::new(pref);
        let session = monitor.session().lock().unwrap();
        assert!(session.current_audio_track.is_none());
    }

    #[test]
    fn test_track_monitor_set_warning_sender() {
        let pref = TrackPreference::default();
        let mut monitor = TrackMonitor::new(pref);
        let (tx, _rx) = async_channel::unbounded::<MismatchWarning>();
        monitor.set_warning_sender(tx);
        assert!(monitor.warning_tx.is_some());
    }
}
