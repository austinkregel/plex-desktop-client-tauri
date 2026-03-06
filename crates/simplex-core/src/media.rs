//! Media session state machine and audio/subtitle track change detection.
//!
//! Tracks playback state, available tracks, and evaluates when to pause
//! based on language preferences.

use serde::{Deserialize, Serialize};

use crate::config::{MismatchAction, UserSettings};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackPreference {
    pub preferred_languages: Vec<String>,
    pub pause_on_mismatch: bool,
    pub mismatch_action: MismatchAction,
}

impl Default for TrackPreference {
    fn default() -> Self {
        Self {
            preferred_languages: vec!["eng".to_string(), "en".to_string()],
            pause_on_mismatch: true,
            mismatch_action: MismatchAction::WarnDialog,
        }
    }
}

impl TrackPreference {
    pub fn from_user_settings(settings: &UserSettings) -> Self {
        let action = settings.audio.language_mismatch_action.clone();
        Self {
            preferred_languages: settings.audio.preferred_languages.clone(),
            pause_on_mismatch: action != MismatchAction::Ignore,
            mismatch_action: action,
        }
    }
}

#[derive(Debug, Clone)]
pub enum TrackEvent {
    AudioTrackChanged {
        index: i32,
        language: Option<String>,
    },
    SubtitleTrackChanged {
        index: i32,
        language: Option<String>,
    },
    PauseRequested {
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum PlaybackState {
    Stopped,
    Playing,
    Paused,
    Buffering,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioTrack {
    pub index: i32,
    pub language: Option<String>,
    pub title: Option<String>,
    pub codec: Option<String>,
    pub channels: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubtitleTrack {
    pub index: i32,
    pub language: Option<String>,
    pub title: Option<String>,
    pub codec: Option<String>,
    pub forced: bool,
}

#[derive(Debug, Clone)]
pub struct MediaSession {
    pub state: PlaybackState,
    pub current_audio_track: Option<AudioTrack>,
    pub current_subtitle_track: Option<SubtitleTrack>,
    pub available_audio_tracks: Vec<AudioTrack>,
    pub available_subtitle_tracks: Vec<SubtitleTrack>,
    pub track_preference: TrackPreference,
    pub position_ms: u64,
    pub duration_ms: u64,
    pub volume: f64,
    pub media_url: Option<String>,
    pub media_title: Option<String>,
}

impl Default for MediaSession {
    fn default() -> Self {
        Self {
            state: PlaybackState::Stopped,
            current_audio_track: None,
            current_subtitle_track: None,
            available_audio_tracks: Vec::new(),
            available_subtitle_tracks: Vec::new(),
            track_preference: TrackPreference::default(),
            position_ms: 0,
            duration_ms: 0,
            volume: 1.0,
            media_url: None,
            media_title: None,
        }
    }
}

impl MediaSession {
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if audio track change requires action based on preferences.
    /// Returns Some(TrackEvent::PauseRequested) if the new track doesn't match
    /// preferred languages and no preferred language track is available.
    pub fn evaluate_audio_track_change(&self, new_track: &AudioTrack) -> Option<TrackEvent> {
        if !self.track_preference.pause_on_mismatch {
            return None;
        }

        let new_lang = match &new_track.language {
            Some(lang) => lang,
            None => return None, // Can't evaluate without language info
        };

        let is_preferred = self
            .track_preference
            .preferred_languages
            .iter()
            .any(|pref| pref.eq_ignore_ascii_case(new_lang));

        if is_preferred {
            return None;
        }

        // Check if any available track has a preferred language
        let has_preferred_track = self.available_audio_tracks.iter().any(|track| {
            track.language.as_ref().map_or(false, |lang| {
                self.track_preference
                    .preferred_languages
                    .iter()
                    .any(|pref| pref.eq_ignore_ascii_case(lang))
            })
        });

        if !has_preferred_track {
            Some(TrackEvent::PauseRequested {
                reason: format!(
                    "Audio switched to {} -- no {} track available",
                    new_lang,
                    self.track_preference.preferred_languages.join("/")
                ),
            })
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_session(
        available_audio_tracks: Vec<AudioTrack>,
        preferred_languages: Vec<&str>,
        pause_on_mismatch: bool,
    ) -> MediaSession {
        let mut session = MediaSession::new();
        session.available_audio_tracks = available_audio_tracks;
        session.track_preference = TrackPreference {
            preferred_languages: preferred_languages
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
            pause_on_mismatch,
            mismatch_action: if pause_on_mismatch {
                MismatchAction::Pause
            } else {
                MismatchAction::Ignore
            },
        };
        session
    }

    #[test]
    fn test_evaluate_audio_track_change_track_matches_preferred() {
        // Track matches preferred language -> returns None
        let session = make_session(
            vec![
                AudioTrack {
                    index: 0,
                    language: Some("eng".to_string()),
                    title: None,
                    codec: None,
                    channels: None,
                },
                AudioTrack {
                    index: 1,
                    language: Some("spa".to_string()),
                    title: None,
                    codec: None,
                    channels: None,
                },
            ],
            vec!["eng", "en"],
            true,
        );

        let new_track = AudioTrack {
            index: 0,
            language: Some("eng".to_string()),
            title: None,
            codec: None,
            channels: None,
        };

        assert!(session.evaluate_audio_track_change(&new_track).is_none());
    }

    #[test]
    fn test_evaluate_audio_track_change_preferred_track_exists() {
        // Track doesn't match but a preferred track exists -> returns None
        let session = make_session(
            vec![
                AudioTrack {
                    index: 0,
                    language: Some("eng".to_string()),
                    title: None,
                    codec: None,
                    channels: None,
                },
                AudioTrack {
                    index: 1,
                    language: Some("spa".to_string()),
                    title: None,
                    codec: None,
                    channels: None,
                },
            ],
            vec!["eng", "en"],
            true,
        );

        let new_track = AudioTrack {
            index: 1,
            language: Some("spa".to_string()),
            title: None,
            codec: None,
            channels: None,
        };

        assert!(session.evaluate_audio_track_change(&new_track).is_none());
    }

    #[test]
    fn test_evaluate_audio_track_change_no_preferred_track_available() {
        // Track doesn't match and no preferred track available -> returns PauseRequested
        let session = make_session(
            vec![
                AudioTrack {
                    index: 0,
                    language: Some("spa".to_string()),
                    title: None,
                    codec: None,
                    channels: None,
                },
                AudioTrack {
                    index: 1,
                    language: Some("fra".to_string()),
                    title: None,
                    codec: None,
                    channels: None,
                },
            ],
            vec!["eng", "en"],
            true,
        );

        let new_track = AudioTrack {
            index: 0,
            language: Some("spa".to_string()),
            title: None,
            codec: None,
            channels: None,
        };

        let result = session.evaluate_audio_track_change(&new_track);
        match &result {
            Some(TrackEvent::PauseRequested { reason }) => {
                assert!(reason.contains("spa"));
                assert!(reason.contains("eng"));
            }
            _ => panic!("Expected PauseRequested, got {:?}", result),
        }
    }

    #[test]
    fn test_evaluate_audio_track_change_pause_on_mismatch_false() {
        // pause_on_mismatch is false -> always returns None
        let session = make_session(
            vec![AudioTrack {
                index: 0,
                language: Some("spa".to_string()),
                title: None,
                codec: None,
                channels: None,
            }],
            vec!["eng", "en"],
            false,
        );

        let new_track = AudioTrack {
            index: 0,
            language: Some("spa".to_string()),
            title: None,
            codec: None,
            channels: None,
        };

        assert!(session.evaluate_audio_track_change(&new_track).is_none());
    }

    #[test]
    fn test_evaluate_audio_track_change_no_language_returns_none() {
        // Track with no language info -> returns None (can't evaluate)
        let session = make_session(
            vec![AudioTrack {
                index: 0,
                language: None,
                title: None,
                codec: None,
                channels: None,
            }],
            vec!["eng", "en"],
            true,
        );

        let new_track = AudioTrack {
            index: 0,
            language: None,
            title: None,
            codec: None,
            channels: None,
        };

        assert!(session.evaluate_audio_track_change(&new_track).is_none());
    }

    #[test]
    fn test_evaluate_audio_track_change_case_insensitive() {
        // Preferred language match is case-insensitive
        let session = make_session(
            vec![AudioTrack {
                index: 0,
                language: Some("ENG".to_string()),
                title: None,
                codec: None,
                channels: None,
            }],
            vec!["eng"],
            true,
        );

        let new_track = AudioTrack {
            index: 0,
            language: Some("ENG".to_string()),
            title: None,
            codec: None,
            channels: None,
        };

        assert!(session.evaluate_audio_track_change(&new_track).is_none());
    }

    // -- TrackPreference::from_user_settings tests --

    #[test]
    fn test_from_user_settings_pause() {
        let mut settings = UserSettings::default();
        settings.audio.language_mismatch_action = MismatchAction::Pause;
        settings.audio.preferred_languages = vec!["jpn".to_string()];

        let pref = TrackPreference::from_user_settings(&settings);
        assert!(pref.pause_on_mismatch);
        assert_eq!(pref.mismatch_action, MismatchAction::Pause);
        assert_eq!(pref.preferred_languages, vec!["jpn"]);
    }

    #[test]
    fn test_from_user_settings_warn_dialog() {
        let mut settings = UserSettings::default();
        settings.audio.language_mismatch_action = MismatchAction::WarnDialog;

        let pref = TrackPreference::from_user_settings(&settings);
        assert!(pref.pause_on_mismatch);
        assert_eq!(pref.mismatch_action, MismatchAction::WarnDialog);
    }

    #[test]
    fn test_from_user_settings_ignore() {
        let mut settings = UserSettings::default();
        settings.audio.language_mismatch_action = MismatchAction::Ignore;

        let pref = TrackPreference::from_user_settings(&settings);
        assert!(!pref.pause_on_mismatch);
        assert_eq!(pref.mismatch_action, MismatchAction::Ignore);
    }

    #[test]
    fn test_from_user_settings_inherits_languages() {
        let mut settings = UserSettings::default();
        settings.audio.preferred_languages =
            vec!["fra".to_string(), "deu".to_string(), "eng".to_string()];

        let pref = TrackPreference::from_user_settings(&settings);
        assert_eq!(pref.preferred_languages, vec!["fra", "deu", "eng"]);
    }

    #[test]
    fn test_from_user_settings_empty_languages() {
        let mut settings = UserSettings::default();
        settings.audio.preferred_languages = Vec::new();

        let pref = TrackPreference::from_user_settings(&settings);
        assert!(pref.preferred_languages.is_empty());
    }

    #[test]
    fn test_track_preference_default_has_warn_dialog() {
        let pref = TrackPreference::default();
        assert_eq!(pref.mismatch_action, MismatchAction::WarnDialog);
        assert!(pref.pause_on_mismatch);
        assert_eq!(pref.preferred_languages, vec!["eng", "en"]);
    }
}
