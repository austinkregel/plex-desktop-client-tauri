//! Pure business logic extracted from the player view for testability.

use simplex_core::api::library::Marker;
use simplex_core::config::MismatchAction;
use simplex_core::media::MediaSession;

use super::pipeline::PipelineApi;
use crate::window::SettingsEvent;

/// Convert seconds to milliseconds, clamping negatives to zero.
pub(crate) fn secs_to_ms(value: f64) -> u64 {
    (value.max(0.0) * 1000.0) as u64
}

/// Which kind of marker the playback position is inside, if any.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ActiveMarker {
    Intro { skip_to_secs: f64 },
    Credits { skip_to_secs: f64 },
}

/// Check whether the current playback position falls inside a marker region.
pub(crate) fn active_marker(markers: &[Marker], position_ms: u64) -> Option<ActiveMarker> {
    for m in markers {
        let (Some(start), Some(end)) = (m.start_time_offset, m.end_time_offset) else {
            continue;
        };
        if position_ms >= start && position_ms < end {
            match m.marker_type.as_deref() {
                Some("intro") => {
                    return Some(ActiveMarker::Intro {
                        skip_to_secs: end as f64 / 1000.0,
                    })
                }
                Some("credits") => {
                    return Some(ActiveMarker::Credits {
                        skip_to_secs: end as f64 / 1000.0,
                    })
                }
                _ => {}
            }
        }
    }
    None
}

/// What the scrobble system should do given the current progress.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum CompletionAction {
    Scrobble,
    Unscrobble,
}

/// Determine whether to scrobble or unscrobble based on playback progress.
pub(crate) fn should_scrobble(
    position: f64,
    duration: f64,
    threshold: f64,
    was_scrobbled: bool,
) -> Option<CompletionAction> {
    if duration <= 0.0 {
        return None;
    }
    let progress = (position / duration).clamp(0.0, 1.0);
    if progress >= threshold {
        Some(CompletionAction::Scrobble)
    } else if was_scrobbled {
        Some(CompletionAction::Unscrobble)
    } else {
        None
    }
}

/// Whether the "Up Next" card should begin showing.
pub(crate) fn should_show_up_next(
    has_next: bool,
    dismissed: bool,
    is_music: bool,
    in_credits: bool,
    position: f64,
    duration: Option<f64>,
) -> bool {
    if !has_next || dismissed || is_music {
        return false;
    }
    in_credits || duration.map_or(false, |d| d > 0.0 && position >= d - 30.0)
}

/// Format the episode metadata line shown in the player controls.
pub(crate) fn format_episode_line(
    grandparent_title: Option<&str>,
    parent_index: Option<u32>,
    index: Option<u32>,
    title: &str,
) -> String {
    if grandparent_title.is_none() {
        return String::new();
    }
    let mut ep = String::new();
    if let (Some(si), Some(ei)) = (parent_index, index) {
        ep.push_str(&format!("S{} \u{00b7} E{}", si, ei));
    }
    if grandparent_title.is_some() {
        if !ep.is_empty() {
            ep.push_str(" \u{2014} ");
        }
        ep.push_str(title);
    }
    ep
}

/// Format the "Up Next" subtitle line (e.g. "S2 · E5").
pub(crate) fn format_up_next_subtitle(parent_index: Option<u32>, index: Option<u32>) -> String {
    match (parent_index, index) {
        (Some(si), Some(ei)) => format!("S{} \u{00b7} E{}", si, ei),
        _ => String::new(),
    }
}

/// Process a settings change event, updating the pipeline and track monitor
/// session as appropriate. Extracted here so the logic can be unit-tested
/// with `MockPipeline` without requiring a running GLib main loop.
pub(crate) fn handle_settings_event(
    pipeline: &impl PipelineApi,
    session: &mut MediaSession,
    event: SettingsEvent,
) {
    match event {
        SettingsEvent::AudioLanguagesChanged(langs) => {
            pipeline.set_preferred_audio_languages(langs.clone());
            pipeline.set_session_audio_override(false);
            session.track_preference.preferred_languages = langs;
        }
        SettingsEvent::AudioMismatchActionChanged(action) => {
            let pause = matches!(action, MismatchAction::Pause | MismatchAction::WarnDialog);
            session.track_preference.mismatch_action = action;
            session.track_preference.pause_on_mismatch = pause;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_marker(marker_type: &str, start: u64, end: u64) -> Marker {
        Marker {
            id: None,
            marker_type: Some(marker_type.to_string()),
            start_time_offset: Some(start),
            end_time_offset: Some(end),
        }
    }

    // ---- secs_to_ms ---------------------------------------------------------

    #[test]
    fn test_secs_to_ms_zero() {
        assert_eq!(secs_to_ms(0.0), 0);
    }

    #[test]
    fn test_secs_to_ms_positive() {
        assert_eq!(secs_to_ms(1.5), 1500);
    }

    #[test]
    fn test_secs_to_ms_negative_clamps_to_zero() {
        assert_eq!(secs_to_ms(-5.0), 0);
    }

    #[test]
    fn test_secs_to_ms_fractional() {
        assert_eq!(secs_to_ms(0.001), 1);
    }

    #[test]
    fn test_secs_to_ms_large() {
        assert_eq!(secs_to_ms(3600.0), 3_600_000);
    }

    // ---- active_marker ------------------------------------------------------

    #[test]
    fn test_active_marker_no_markers() {
        assert_eq!(active_marker(&[], 5000), None);
    }

    #[test]
    fn test_active_marker_in_intro() {
        let markers = vec![make_marker("intro", 0, 30000)];
        let result = active_marker(&markers, 15000);
        assert_eq!(result, Some(ActiveMarker::Intro { skip_to_secs: 30.0 }));
    }

    #[test]
    fn test_active_marker_in_credits() {
        let markers = vec![make_marker("credits", 3500000, 3600000)];
        let result = active_marker(&markers, 3550000);
        assert_eq!(
            result,
            Some(ActiveMarker::Credits {
                skip_to_secs: 3600.0
            })
        );
    }

    #[test]
    fn test_active_marker_outside_range() {
        let markers = vec![make_marker("intro", 0, 30000)];
        assert_eq!(active_marker(&markers, 30000), None);
        assert_eq!(active_marker(&markers, 50000), None);
    }

    #[test]
    fn test_active_marker_at_exact_start() {
        let markers = vec![make_marker("intro", 10000, 20000)];
        assert_eq!(
            active_marker(&markers, 10000),
            Some(ActiveMarker::Intro { skip_to_secs: 20.0 })
        );
    }

    #[test]
    fn test_active_marker_at_exact_end_excluded() {
        let markers = vec![make_marker("intro", 10000, 20000)];
        assert_eq!(active_marker(&markers, 20000), None);
    }

    #[test]
    fn test_active_marker_missing_offsets() {
        let marker = Marker {
            id: None,
            marker_type: Some("intro".to_string()),
            start_time_offset: None,
            end_time_offset: Some(30000),
        };
        assert_eq!(active_marker(&[marker], 15000), None);
    }

    #[test]
    fn test_active_marker_unknown_type_ignored() {
        let markers = vec![make_marker("chapter", 0, 30000)];
        assert_eq!(active_marker(&markers, 15000), None);
    }

    #[test]
    fn test_active_marker_intro_takes_priority_over_later_markers() {
        let markers = vec![
            make_marker("intro", 0, 30000),
            make_marker("credits", 0, 30000),
        ];
        let result = active_marker(&markers, 15000);
        assert_eq!(result, Some(ActiveMarker::Intro { skip_to_secs: 30.0 }));
    }

    // ---- should_scrobble ----------------------------------------------------

    #[test]
    fn test_should_scrobble_above_threshold() {
        assert_eq!(
            should_scrobble(91.0, 100.0, 0.90, false),
            Some(CompletionAction::Scrobble)
        );
    }

    #[test]
    fn test_should_scrobble_at_threshold() {
        assert_eq!(
            should_scrobble(90.0, 100.0, 0.90, false),
            Some(CompletionAction::Scrobble)
        );
    }

    #[test]
    fn test_should_scrobble_below_threshold_not_scrobbled() {
        assert_eq!(should_scrobble(50.0, 100.0, 0.90, false), None);
    }

    #[test]
    fn test_should_scrobble_below_threshold_was_scrobbled_unscrobbles() {
        assert_eq!(
            should_scrobble(50.0, 100.0, 0.90, true),
            Some(CompletionAction::Unscrobble)
        );
    }

    #[test]
    fn test_should_scrobble_zero_duration() {
        assert_eq!(should_scrobble(50.0, 0.0, 0.90, false), None);
    }

    #[test]
    fn test_should_scrobble_negative_duration() {
        assert_eq!(should_scrobble(50.0, -10.0, 0.90, false), None);
    }

    #[test]
    fn test_should_scrobble_position_exceeds_duration() {
        assert_eq!(
            should_scrobble(150.0, 100.0, 0.90, false),
            Some(CompletionAction::Scrobble)
        );
    }

    // ---- should_show_up_next ------------------------------------------------

    #[test]
    fn test_up_next_no_next() {
        assert!(!should_show_up_next(
            false,
            false,
            false,
            false,
            100.0,
            Some(120.0)
        ));
    }

    #[test]
    fn test_up_next_dismissed() {
        assert!(!should_show_up_next(
            true,
            true,
            false,
            true,
            100.0,
            Some(120.0)
        ));
    }

    #[test]
    fn test_up_next_music_suppressed() {
        assert!(!should_show_up_next(
            true,
            false,
            true,
            true,
            100.0,
            Some(120.0)
        ));
    }

    #[test]
    fn test_up_next_in_credits() {
        assert!(should_show_up_next(
            true,
            false,
            false,
            true,
            50.0,
            Some(120.0)
        ));
    }

    #[test]
    fn test_up_next_near_end() {
        assert!(should_show_up_next(
            true,
            false,
            false,
            false,
            95.0,
            Some(120.0)
        ));
    }

    #[test]
    fn test_up_next_not_near_end() {
        assert!(!should_show_up_next(
            true,
            false,
            false,
            false,
            50.0,
            Some(120.0)
        ));
    }

    #[test]
    fn test_up_next_no_duration() {
        assert!(!should_show_up_next(true, false, false, false, 95.0, None));
    }

    // ---- format_episode_line ------------------------------------------------

    #[test]
    fn test_format_episode_line_full() {
        let line = format_episode_line(Some("Breaking Bad"), Some(2), Some(5), "Mandala");
        assert_eq!(line, "S2 \u{00b7} E5 \u{2014} Mandala");
    }

    #[test]
    fn test_format_episode_line_no_indices() {
        let line = format_episode_line(Some("Breaking Bad"), None, None, "Mandala");
        assert_eq!(line, "Mandala");
    }

    #[test]
    fn test_format_episode_line_no_grandparent() {
        let line = format_episode_line(None, Some(1), Some(1), "Pilot");
        assert_eq!(line, "");
    }

    #[test]
    fn test_format_episode_line_partial_index() {
        let line = format_episode_line(Some("Show"), Some(3), None, "Title");
        assert_eq!(line, "Title");
    }

    // ---- format_up_next_subtitle --------------------------------------------

    #[test]
    fn test_format_up_next_subtitle_full() {
        assert_eq!(format_up_next_subtitle(Some(2), Some(5)), "S2 \u{00b7} E5");
    }

    #[test]
    fn test_format_up_next_subtitle_missing() {
        assert_eq!(format_up_next_subtitle(None, Some(5)), "");
        assert_eq!(format_up_next_subtitle(Some(2), None), "");
        assert_eq!(format_up_next_subtitle(None, None), "");
    }

    // ---- handle_settings_event -----------------------------------------------

    use crate::player::pipeline::mock::MockPipeline;
    use simplex_core::media::TrackPreference;

    fn make_session() -> MediaSession {
        MediaSession {
            track_preference: TrackPreference {
                preferred_languages: vec!["eng".to_string()],
                pause_on_mismatch: true,
                mismatch_action: MismatchAction::WarnDialog,
            },
            ..Default::default()
        }
    }

    #[test]
    fn test_handle_audio_languages_changed_updates_pipeline() {
        let mock = MockPipeline::new();
        let mut session = make_session();
        let event = SettingsEvent::AudioLanguagesChanged(vec!["jpn".to_string()]);
        handle_settings_event(&mock, &mut session, event);
        assert_eq!(mock.preferred_audio_languages(), vec!["jpn".to_string()]);
    }

    #[test]
    fn test_handle_audio_languages_changed_updates_session() {
        let mock = MockPipeline::new();
        let mut session = make_session();
        let event = SettingsEvent::AudioLanguagesChanged(vec!["spa".to_string(), "es".to_string()]);
        handle_settings_event(&mock, &mut session, event);
        assert_eq!(
            session.track_preference.preferred_languages,
            vec!["spa".to_string(), "es".to_string()]
        );
    }

    #[test]
    fn test_handle_audio_languages_changed_clears_session_override() {
        let mock = MockPipeline::new();
        mock.set_session_audio_override(true);
        let mut session = make_session();
        let event = SettingsEvent::AudioLanguagesChanged(vec!["jpn".to_string()]);
        handle_settings_event(&mock, &mut session, event);
        assert!(!mock.has_session_audio_override());
    }

    #[test]
    fn test_handle_mismatch_action_changed_pause() {
        let mock = MockPipeline::new();
        let mut session = make_session();
        let event = SettingsEvent::AudioMismatchActionChanged(MismatchAction::Pause);
        handle_settings_event(&mock, &mut session, event);
        assert_eq!(session.track_preference.mismatch_action, MismatchAction::Pause);
        assert!(session.track_preference.pause_on_mismatch);
    }

    #[test]
    fn test_handle_mismatch_action_changed_ignore() {
        let mock = MockPipeline::new();
        let mut session = make_session();
        let event = SettingsEvent::AudioMismatchActionChanged(MismatchAction::Ignore);
        handle_settings_event(&mock, &mut session, event);
        assert_eq!(session.track_preference.mismatch_action, MismatchAction::Ignore);
        assert!(!session.track_preference.pause_on_mismatch);
    }

    #[test]
    fn test_handle_mismatch_action_changed_warn_dialog() {
        let mock = MockPipeline::new();
        let mut session = make_session();
        session.track_preference.pause_on_mismatch = false;
        let event = SettingsEvent::AudioMismatchActionChanged(MismatchAction::WarnDialog);
        handle_settings_event(&mock, &mut session, event);
        assert_eq!(session.track_preference.mismatch_action, MismatchAction::WarnDialog);
        assert!(session.track_preference.pause_on_mismatch);
    }

    #[test]
    fn test_audio_change_does_not_affect_subtitle_settings() {
        let mock = MockPipeline::new();
        let mut session = make_session();
        session.current_subtitle_track = Some(simplex_core::media::SubtitleTrack {
            index: 1,
            language: Some("eng".to_string()),
            title: Some("English".to_string()),
        });
        let event = SettingsEvent::AudioLanguagesChanged(vec!["jpn".to_string()]);
        handle_settings_event(&mock, &mut session, event);
        assert!(session.current_subtitle_track.is_some());
        assert_eq!(
            session.current_subtitle_track.as_ref().unwrap().language,
            Some("eng".to_string())
        );
    }

    #[test]
    fn test_mismatch_change_does_not_affect_audio_languages() {
        let mock = MockPipeline::new();
        let mut session = make_session();
        let event = SettingsEvent::AudioMismatchActionChanged(MismatchAction::Ignore);
        handle_settings_event(&mock, &mut session, event);
        assert_eq!(
            session.track_preference.preferred_languages,
            vec!["eng".to_string()]
        );
    }
}
