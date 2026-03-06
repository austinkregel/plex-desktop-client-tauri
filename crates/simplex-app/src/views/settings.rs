//! Application-wide settings page using adw::PreferencesPage.

use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Entry, Orientation, StringList, Switch};
use libadwaita::prelude::*;
use libadwaita::{ActionRow, ComboRow, PreferencesGroup, PreferencesPage};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use crate::window::{AppState, SettingsEvent};
use simplex_core::config::{self, MismatchAction, StreamQuality, SubtitleAutoEnable, UserSettings};

pub struct SettingsView {
    pub widget: GtkBox,
    pub(crate) audio_language_entry: Entry,
    pub(crate) mismatch_action_row: ComboRow,
    pub(crate) subtitle_language_entry: Entry,
    pub(crate) subtitle_auto_enable_row: ComboRow,
}

pub fn build(state: Arc<Mutex<AppState>>) -> SettingsView {
    let container = GtkBox::new(Orientation::Vertical, 0);
    container.set_vexpand(true);
    container.set_hexpand(true);

    let settings = Rc::new(RefCell::new(config::load_user_settings()));
    let event_tx = state.lock().unwrap().settings_event_tx.clone();

    let page = PreferencesPage::new();
    page.set_title("Settings");

    page.add(&build_playback_group(&settings));

    let (audio_group, audio_language_entry, mismatch_action_row) =
        build_audio_group(&settings, &event_tx);
    page.add(&audio_group);

    let (subtitle_group, subtitle_language_entry, subtitle_auto_enable_row) =
        build_subtitle_group(&settings);
    page.add(&subtitle_group);

    page.add(&build_server_group());

    container.append(&page);
    SettingsView {
        widget: container,
        audio_language_entry,
        mismatch_action_row,
        subtitle_language_entry,
        subtitle_auto_enable_row,
    }
}

// ── Playback ──────────────────────────────────────────────────────────────

fn build_playback_group(settings: &Rc<RefCell<UserSettings>>) -> PreferencesGroup {
    let group = PreferencesGroup::new();
    group.set_title("Playback");

    // Stream quality
    {
        let row = ComboRow::new();
        row.set_title("Streaming Quality");
        row.set_subtitle("Maximum bitrate for remote streams");

        let quality_labels: Vec<&str> = std::iter::once("Original")
            .chain(StreamQuality::PRESETS.iter().map(|(_, l)| *l))
            .collect();
        let model = StringList::new(&quality_labels);
        row.set_model(Some(&model));

        let current = &settings.borrow().playback.quality;
        let active = match current {
            StreamQuality::Original => 0u32,
            StreamQuality::Maximum(kbps) => StreamQuality::PRESETS
                .iter()
                .position(|(k, _)| k == kbps)
                .map(|i| (i + 1) as u32)
                .unwrap_or(0),
        };
        row.set_selected(active);

        let s = settings.clone();
        row.connect_selected_notify(move |r| {
            let idx = r.selected() as usize;
            let quality = if idx == 0 {
                StreamQuality::Original
            } else {
                let (kbps, _) = StreamQuality::PRESETS[idx - 1];
                StreamQuality::Maximum(kbps)
            };
            s.borrow_mut().playback.quality = quality;
            save(&s.borrow());
        });
        group.add(&row);
    }

    // Auto-adjust quality
    {
        let row = ActionRow::new();
        row.set_title("Auto-Adjust Quality");
        row.set_subtitle("Lower quality automatically on slow connections");
        let toggle = Switch::new();
        toggle.set_valign(gtk4::Align::Center);
        toggle.set_active(settings.borrow().playback.auto_adjust_quality);
        let s = settings.clone();
        toggle.connect_state_set(move |_, active| {
            s.borrow_mut().playback.auto_adjust_quality = active;
            save(&s.borrow());
            glib::Propagation::Proceed
        });
        row.add_suffix(&toggle);
        row.set_activatable_widget(Some(&toggle));
        group.add(&row);
    }

    // Preferred codec
    {
        let row = ActionRow::new();
        row.set_title("Preferred Codec");
        row.set_subtitle("e.g. h264, hevc, av1 — leave empty for auto");
        let entry = Entry::new();
        entry.set_valign(gtk4::Align::Center);
        entry.set_width_chars(8);
        if let Some(ref codec) = settings.borrow().playback.preferred_codec {
            entry.set_text(codec);
        }
        let s = settings.clone();
        entry.connect_changed(move |e| {
            let text = e.text().to_string();
            s.borrow_mut().playback.preferred_codec =
                if text.is_empty() { None } else { Some(text) };
            save(&s.borrow());
        });
        row.add_suffix(&entry);
        group.add(&row);
    }

    // Playback speed
    {
        let row = ComboRow::new();
        row.set_title("Default Playback Speed");
        let labels = StringList::new(&["0.5x", "0.75x", "1.0x", "1.25x", "1.5x", "2.0x"]);
        row.set_model(Some(&labels));
        let speeds = [0.5, 0.75, 1.0, 1.25, 1.5, 2.0];
        let current_speed = settings.borrow().playback.playback_speed;
        let active = speeds
            .iter()
            .position(|&s| (s - current_speed).abs() < 0.01)
            .unwrap_or(2) as u32;
        row.set_selected(active);

        let s = settings.clone();
        row.connect_selected_notify(move |r| {
            let idx = r.selected() as usize;
            s.borrow_mut().playback.playback_speed = speeds[idx];
            save(&s.borrow());
        });
        group.add(&row);
    }

    // Remember volume
    {
        let row = ActionRow::new();
        row.set_title("Remember Volume");
        row.set_subtitle("Persist volume level across sessions");
        let toggle = Switch::new();
        toggle.set_valign(gtk4::Align::Center);
        toggle.set_active(settings.borrow().playback.remember_volume);
        let s = settings.clone();
        toggle.connect_state_set(move |_, active| {
            s.borrow_mut().playback.remember_volume = active;
            save(&s.borrow());
            glib::Propagation::Proceed
        });
        row.add_suffix(&toggle);
        row.set_activatable_widget(Some(&toggle));
        group.add(&row);
    }

    group
}

// ── Audio ─────────────────────────────────────────────────────────────────

fn build_audio_group(
    settings: &Rc<RefCell<UserSettings>>,
    event_tx: &async_channel::Sender<SettingsEvent>,
) -> (PreferencesGroup, Entry, ComboRow) {
    let group = PreferencesGroup::new();
    group.set_title("Audio");

    // Preferred languages
    let lang_entry = Entry::new();
    {
        let row = ActionRow::new();
        row.set_title("Preferred Languages");
        row.set_subtitle("Comma-separated ISO 639 codes (e.g. eng, jpn)");
        lang_entry.set_valign(gtk4::Align::Center);
        lang_entry.set_width_chars(14);
        lang_entry.set_text(&settings.borrow().audio.preferred_languages.join(", "));
        let s = settings.clone();
        let tx = event_tx.clone();
        lang_entry.connect_changed(move |e| {
            let text = e.text().to_string();
            let langs: Vec<String> = text
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            s.borrow_mut().audio.preferred_languages = langs.clone();
            save(&s.borrow());
            let _ = tx.try_send(SettingsEvent::AudioLanguagesChanged(langs));
        });
        row.add_suffix(&lang_entry);
        group.add(&row);
    }

    // Language mismatch action
    let mismatch_row = ComboRow::new();
    {
        mismatch_row.set_title("On Language Mismatch");
        mismatch_row.set_subtitle("Action when preferred audio language is unavailable");
        let labels = StringList::new(&["Show Warning Dialog", "Pause Playback", "Ignore"]);
        mismatch_row.set_model(Some(&labels));
        let active = match settings.borrow().audio.language_mismatch_action {
            MismatchAction::WarnDialog => 0u32,
            MismatchAction::Pause => 1,
            MismatchAction::Ignore => 2,
        };
        mismatch_row.set_selected(active);

        let s = settings.clone();
        let tx = event_tx.clone();
        mismatch_row.connect_selected_notify(move |r| {
            let action = match r.selected() {
                0 => MismatchAction::WarnDialog,
                1 => MismatchAction::Pause,
                _ => MismatchAction::Ignore,
            };
            s.borrow_mut().audio.language_mismatch_action = action.clone();
            save(&s.borrow());
            let _ = tx.try_send(SettingsEvent::AudioMismatchActionChanged(action));
        });
        group.add(&mismatch_row);
    }

    (group, lang_entry, mismatch_row)
}

// ── Subtitles ─────────────────────────────────────────────────────────────

fn build_subtitle_group(
    settings: &Rc<RefCell<UserSettings>>,
) -> (PreferencesGroup, Entry, ComboRow) {
    let group = PreferencesGroup::new();
    group.set_title("Subtitles");

    // Preferred languages
    let lang_entry = Entry::new();
    {
        let row = ActionRow::new();
        row.set_title("Preferred Languages");
        row.set_subtitle("Comma-separated ISO 639 codes");
        lang_entry.set_valign(gtk4::Align::Center);
        lang_entry.set_width_chars(14);
        lang_entry.set_text(&settings.borrow().subtitles.preferred_languages.join(", "));
        let s = settings.clone();
        lang_entry.connect_changed(move |e| {
            let text = e.text().to_string();
            let langs: Vec<String> = text
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            s.borrow_mut().subtitles.preferred_languages = langs;
            save(&s.borrow());
        });
        row.add_suffix(&lang_entry);
        group.add(&row);
    }

    // Auto-enable subtitles
    let auto_enable_row = ComboRow::new();
    {
        auto_enable_row.set_title("Auto-Enable Subtitles");
        let labels = StringList::new(&["Always", "On Language Mismatch", "Never"]);
        auto_enable_row.set_model(Some(&labels));
        let active = match settings.borrow().subtitles.auto_enable {
            SubtitleAutoEnable::Always => 0u32,
            SubtitleAutoEnable::OnMismatch => 1,
            SubtitleAutoEnable::Never => 2,
        };
        auto_enable_row.set_selected(active);

        let s = settings.clone();
        auto_enable_row.connect_selected_notify(move |r| {
            let mode = match r.selected() {
                0 => SubtitleAutoEnable::Always,
                1 => SubtitleAutoEnable::OnMismatch,
                _ => SubtitleAutoEnable::Never,
            };
            s.borrow_mut().subtitles.auto_enable = mode;
            save(&s.borrow());
        });
        group.add(&auto_enable_row);
    }

    // Prefer forced subtitles
    {
        let row = ActionRow::new();
        row.set_title("Prefer Forced Subtitles");
        row.set_subtitle("Use forced subtitle tracks when available");
        let toggle = Switch::new();
        toggle.set_valign(gtk4::Align::Center);
        toggle.set_active(settings.borrow().subtitles.prefer_forced);
        let s = settings.clone();
        toggle.connect_state_set(move |_, active| {
            s.borrow_mut().subtitles.prefer_forced = active;
            save(&s.borrow());
            glib::Propagation::Proceed
        });
        row.add_suffix(&toggle);
        row.set_activatable_widget(Some(&toggle));
        group.add(&row);
    }

    (group, lang_entry, auto_enable_row)
}

// ── Server ────────────────────────────────────────────────────────────────

fn build_server_group() -> PreferencesGroup {
    let group = PreferencesGroup::new();
    group.set_title("Server");
    group.set_description(Some("Connected Plex Media Servers"));

    let cfg = config::load_config();
    if cfg.servers.is_empty() {
        let row = ActionRow::new();
        row.set_title("No servers configured");
        row.set_subtitle("Log in to connect to a Plex server");
        group.add(&row);
    } else {
        for server in &cfg.servers {
            let row = ActionRow::new();
            row.set_title(&server.name);
            row.set_subtitle(&server.base_url);
            if cfg.default_server_id.as_deref() == Some(server.id.as_str()) {
                row.add_suffix(&gtk4::Image::from_icon_name("emblem-default-symbolic"));
            }
            group.add(&row);
        }
    }

    group
}

// ── Helpers ───────────────────────────────────────────────────────────────

fn save(settings: &UserSettings) {
    if let Err(e) = config::save_user_settings(settings) {
        tracing::warn!("Failed to save settings: {e}");
    }
}
