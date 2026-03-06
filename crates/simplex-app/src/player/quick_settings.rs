//! In-player quick-action popover for audio/subtitle track selection and
//! playback speed adjustment.

use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Button, CheckButton, Label, Orientation, Popover, Separator};
use std::sync::{Arc, Mutex};

use super::pipeline::PlayerPipeline;
use super::subtitles::SubtitleManager;

const SPEED_PRESETS: &[(f64, &str)] = &[
    (0.5, "0.5x"),
    (0.75, "0.75x"),
    (1.0, "1.0x"),
    (1.25, "1.25x"),
    (1.5, "1.5x"),
    (2.0, "2.0x"),
];

/// Build the quick-settings popover and its anchor button.
/// Returns `(button, popover)` -- the caller appends the button to the controls
/// and the popover is parented to the button automatically.
pub fn build(pipeline: &Arc<Mutex<PlayerPipeline>>) -> (Button, Popover) {
    let button = Button::from_icon_name("emblem-system-symbolic");
    button.add_css_class("flat");
    button.set_tooltip_text(Some("Audio, Subtitles & Speed"));

    let popover = Popover::new();
    popover.set_parent(&button);

    let pipe = pipeline.clone();
    let pop = popover.clone();
    button.connect_clicked(move |_| {
        refresh_popover_content(&pop, &pipe);
        pop.popup();
    });

    (button, popover)
}

/// Rebuild the popover content each time it's opened so the track lists
/// reflect the current stream state.
fn refresh_popover_content(popover: &Popover, pipeline: &Arc<Mutex<PlayerPipeline>>) {
    let content = GtkBox::new(Orientation::Vertical, 4);
    content.set_margin_start(8);
    content.set_margin_end(8);
    content.set_margin_top(8);
    content.set_margin_bottom(8);
    content.set_width_request(260);

    let p = pipeline.lock().unwrap();

    build_audio_section(&content, &p, pipeline);
    content.append(&Separator::new(Orientation::Horizontal));
    build_subtitle_section(&content, &p, pipeline);
    content.append(&Separator::new(Orientation::Horizontal));
    build_speed_section(&content, pipeline);

    popover.set_child(Some(&content));
}

// ---- Audio track selector ------------------------------------------------

fn build_audio_section(
    container: &GtkBox,
    pipeline: &PlayerPipeline,
    pipe_arc: &Arc<Mutex<PlayerPipeline>>,
) {
    let heading = Label::new(Some("Audio"));
    heading.add_css_class("heading");
    heading.set_xalign(0.0);
    container.append(&heading);

    let count = pipeline.audio_track_count();
    tracing::debug!(
        "Audio track count: {}, playing: {}",
        count,
        pipeline.is_playing()
    );
    if count == 0 {
        let empty = Label::new(Some("No audio tracks detected"));
        empty.add_css_class("dim-label");
        container.append(&empty);
        return;
    }

    let current = pipeline.current_audio_track();
    let mut group: Option<CheckButton> = None;

    for i in 0..count {
        let lang = pipeline.audio_language(i);
        let title = pipeline.audio_title(i);
        let codec = pipeline.audio_codec(i);
        let label_text = audio_track_label(i, lang.as_deref(), title.as_deref(), codec.as_deref());

        let radio = CheckButton::with_label(&label_text);
        if let Some(ref g) = group {
            radio.set_group(Some(g));
        } else {
            group = Some(radio.clone());
        }
        radio.set_active(i == current);

        let pipe = pipe_arc.clone();
        let selected_lang = lang.clone();
        radio.connect_toggled(move |btn| {
            if btn.is_active() {
                let p = pipe.lock().unwrap();
                p.set_audio_track(i);
                if let Some(ref lang) = selected_lang {
                    p.set_preferred_audio_languages(vec![lang.clone()]);
                }
                p.set_session_audio_override(true);
            }
        });
        container.append(&radio);
    }
}

// ---- Subtitle track selector ---------------------------------------------

fn build_subtitle_section(
    container: &GtkBox,
    pipeline: &PlayerPipeline,
    pipe_arc: &Arc<Mutex<PlayerPipeline>>,
) {
    let heading = Label::new(Some("Subtitles"));
    heading.add_css_class("heading");
    heading.set_xalign(0.0);
    container.append(&heading);

    let tracks = SubtitleManager::list_tracks(pipeline);
    let current = pipeline.current_subtitle_track();

    let off_radio = CheckButton::with_label("Off");
    off_radio.set_active(current < 0);

    let pipe_off = pipe_arc.clone();
    off_radio.connect_toggled(move |btn| {
        if btn.is_active() {
            let p = pipe_off.lock().unwrap();
            SubtitleManager::disable(&*p);
        }
    });
    container.append(&off_radio);

    for info in &tracks {
        let label_text = track_label(info.index, info.language.as_deref(), info.title.as_deref());
        let radio = CheckButton::with_label(&label_text);
        radio.set_group(Some(&off_radio));
        radio.set_active(info.index == current);

        let pipe = pipe_arc.clone();
        let idx = info.index;
        radio.connect_toggled(move |btn| {
            if btn.is_active() {
                let p = pipe.lock().unwrap();
                SubtitleManager::select_track(&*p, idx);
            }
        });
        container.append(&radio);
    }
}

// ---- Playback speed selector ---------------------------------------------

fn build_speed_section(container: &GtkBox, pipe_arc: &Arc<Mutex<PlayerPipeline>>) {
    let heading = Label::new(Some("Speed"));
    heading.add_css_class("heading");
    heading.set_xalign(0.0);
    container.append(&heading);

    let speed_box = GtkBox::new(Orientation::Horizontal, 4);
    speed_box.set_halign(gtk4::Align::Fill);

    for &(rate, label) in SPEED_PRESETS {
        let btn = Button::with_label(label);
        btn.add_css_class("flat");
        btn.set_hexpand(true);

        let pipe = pipe_arc.clone();
        btn.connect_clicked(move |_| {
            let p = pipe.lock().unwrap();
            p.set_playback_speed(rate);
        });
        speed_box.append(&btn);
    }
    container.append(&speed_box);
}

// ---- Helpers -------------------------------------------------------------

pub(crate) fn track_label(index: i32, language: Option<&str>, title: Option<&str>) -> String {
    match (language, title) {
        (Some(lang), Some(t)) => format!("{} — {}", lang.to_uppercase(), t),
        (Some(lang), None) => lang.to_uppercase(),
        (None, Some(t)) => t.to_string(),
        (None, None) => format!("Track {}", index + 1),
    }
}

pub(crate) fn audio_track_label(
    index: i32,
    language: Option<&str>,
    title: Option<&str>,
    codec: Option<&str>,
) -> String {
    let base = match (language, title) {
        (Some(lang), Some(t)) => format!("{} — {}", lang.to_uppercase(), t),
        (Some(lang), None) => lang.to_uppercase(),
        (None, Some(t)) => t.to_string(),
        (None, None) => format!("Track {}", index + 1),
    };
    match codec {
        Some(c) if language.is_none() && title.is_none() => format!("{} ({})", base, c),
        _ => base,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- track_label --------------------------------------------------------

    #[test]
    fn test_track_label_lang_and_title() {
        assert_eq!(
            track_label(0, Some("eng"), Some("English")),
            "ENG — English"
        );
    }

    #[test]
    fn test_track_label_lang_only() {
        assert_eq!(track_label(0, Some("spa"), None), "SPA");
    }

    #[test]
    fn test_track_label_title_only() {
        assert_eq!(track_label(0, None, Some("Commentary")), "Commentary");
    }

    #[test]
    fn test_track_label_neither() {
        assert_eq!(track_label(0, None, None), "Track 1");
        assert_eq!(track_label(4, None, None), "Track 5");
    }

    #[test]
    fn test_track_label_uppercase() {
        assert_eq!(track_label(0, Some("fre"), None), "FRE");
    }

    // ---- audio_track_label --------------------------------------------------

    #[test]
    fn test_audio_track_label_lang_and_title_ignores_codec() {
        assert_eq!(
            audio_track_label(0, Some("eng"), Some("Surround"), Some("AAC")),
            "ENG — Surround"
        );
    }

    #[test]
    fn test_audio_track_label_lang_only_ignores_codec() {
        assert_eq!(audio_track_label(0, Some("eng"), None, Some("AAC")), "ENG");
    }

    #[test]
    fn test_audio_track_label_no_lang_no_title_shows_codec() {
        assert_eq!(
            audio_track_label(0, None, None, Some("AAC")),
            "Track 1 (AAC)"
        );
    }

    #[test]
    fn test_audio_track_label_no_lang_no_title_no_codec() {
        assert_eq!(audio_track_label(2, None, None, None), "Track 3");
    }

    #[test]
    fn test_audio_track_label_title_only_ignores_codec() {
        assert_eq!(
            audio_track_label(0, None, Some("Director's Commentary"), Some("MP3")),
            "Director's Commentary"
        );
    }
}
