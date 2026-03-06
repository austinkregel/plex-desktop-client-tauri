//! Player controls overlay with seek bar, metadata, transport, and utility buttons.

use gtk4::prelude::*;
use gtk4::{
    Box as GtkBox, Button, Label, Orientation, Overlay, Picture, Popover, Revealer,
    RevealerTransitionType, Scale, WindowHandle,
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use super::pipeline::PlayerPipeline;
use super::quick_settings;
use simplex_core::ui_utils::format_time;

pub struct PlayerControls {
    pub widget: WindowHandle,
    pub controls_bar: GtkBox,
    pub play_pause_button: Button,
    pub seek_bar: Scale,
    pub volume_scale: Scale,
    pub pip_button: Button,
    pub back_button: Button,
    pub fullscreen_button: Button,
    pub title_label: Label,
    pub position_label: Label,
    pub duration_label: Label,
    pub show_label: Label,
    pub episode_label: Label,
    pub prev_button: Button,
    pub next_button: Button,
    pub stop_button: Button,
    pub skip_back_button: Button,
    pub skip_forward_button: Button,
    pub quick_settings_button: Button,
    pub quick_settings_popover: Popover,
    updating_seek: Rc<Cell<bool>>,
    pub skip_action_revealer: Revealer,
    pub skip_action_button: Button,
    pub up_next_revealer: Revealer,
    pub up_next_title: Label,
    pub up_next_subtitle: Label,
    pub up_next_thumb: Picture,
    pub up_next_countdown: Label,
    pub up_next_play_button: Button,
    pub up_next_cancel_button: Button,
}

fn pip_icon_svg() -> gtk4::Image {
    let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16">
  <rect x="1" y="1" width="14" height="14" rx="1.5" ry="1.5" fill="none" stroke="white" stroke-width="1.5"/>
  <rect x="8" y="8" width="6" height="5" rx="1" ry="1" fill="white"/>
  <path d="M7 4L4 7V5H4L4 4H7Z" fill="white"/>
</svg>"#;
    let bytes = glib::Bytes::from(svg.as_bytes());
    let texture = gdk4::Texture::from_bytes(&bytes).ok();
    let image = gtk4::Image::new();
    if let Some(tex) = texture {
        image.set_paintable(Some(&tex));
    } else {
        image.set_icon_name(Some("view-dual-symbolic"));
    }
    image
}

impl PlayerControls {
    pub fn new(pipeline: &Arc<Mutex<PlayerPipeline>>) -> Self {
        let overlay = Overlay::new();
        overlay.set_vexpand(true);
        overlay.set_hexpand(true);

        {
            let p = pipeline.lock().unwrap();
            overlay.set_child(Some(&p.picture));
        }

        // --- Top bar: back, title, fullscreen ---
        let top_revealer = Revealer::new();
        top_revealer.set_transition_type(RevealerTransitionType::SlideDown);
        top_revealer.set_reveal_child(true);
        top_revealer.set_valign(gtk4::Align::Start);

        let top_bar = GtkBox::new(Orientation::Horizontal, 8);
        top_bar.add_css_class("osd");
        top_bar.add_css_class("toolbar");

        let back_button = Button::from_icon_name("go-previous-symbolic");
        back_button.add_css_class("flat");
        back_button.set_tooltip_text(Some("Back"));

        let title_label = Label::new(None);
        title_label.add_css_class("heading");
        title_label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        title_label.set_hexpand(true);
        title_label.set_xalign(0.0);

        let fullscreen_button = Button::from_icon_name("view-fullscreen-symbolic");
        fullscreen_button.add_css_class("flat");
        fullscreen_button.set_tooltip_text(Some("Toggle Fullscreen (F11)"));

        top_bar.append(&back_button);
        top_bar.append(&title_label);
        top_bar.append(&fullscreen_button);
        top_revealer.set_child(Some(&top_bar));
        overlay.add_overlay(&top_revealer);

        // --- Bottom bar: seek row + 3-column button row ---
        let bottom_revealer = Revealer::new();
        bottom_revealer.set_transition_type(RevealerTransitionType::SlideUp);
        bottom_revealer.set_reveal_child(true);
        bottom_revealer.set_valign(gtk4::Align::End);

        let controls_bar = GtkBox::new(Orientation::Vertical, 4);
        controls_bar.add_css_class("osd");
        controls_bar.add_css_class("toolbar");

        // Seek row
        let seek_row = GtkBox::new(Orientation::Horizontal, 8);
        let position_label = Label::new(Some("0:00"));
        position_label.add_css_class("numeric");
        let seek_bar = Scale::with_range(Orientation::Horizontal, 0.0, 100.0, 1.0);
        seek_bar.set_hexpand(true);
        seek_bar.set_draw_value(false);
        let duration_label = Label::new(Some("0:00"));
        duration_label.add_css_class("numeric");
        seek_row.append(&position_label);
        seek_row.append(&seek_bar);
        seek_row.append(&duration_label);
        controls_bar.append(&seek_row);

        // 3-column button row
        let button_row = GtkBox::new(Orientation::Horizontal, 0);

        // -- Left column: metadata --
        let meta_box = GtkBox::new(Orientation::Vertical, 0);
        meta_box.set_halign(gtk4::Align::Start);
        meta_box.set_hexpand(true);
        meta_box.set_valign(gtk4::Align::Center);

        let show_label = Label::new(None);
        show_label.add_css_class("caption-heading");
        show_label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        show_label.set_xalign(0.0);
        show_label.set_max_width_chars(30);

        let episode_label = Label::new(None);
        episode_label.add_css_class("dim-label");
        episode_label.add_css_class("caption");
        episode_label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        episode_label.set_xalign(0.0);
        episode_label.set_max_width_chars(35);

        meta_box.append(&show_label);
        meta_box.append(&episode_label);

        // -- Center column: transport controls --
        let transport_box = GtkBox::new(Orientation::Horizontal, 4);
        transport_box.set_halign(gtk4::Align::Center);

        let prev_button = Button::from_icon_name("media-skip-backward-symbolic");
        prev_button.add_css_class("flat");
        prev_button.set_tooltip_text(Some("Previous Episode"));
        prev_button.set_sensitive(false);

        let skip_back_button = Button::from_icon_name("media-seek-backward-symbolic");
        skip_back_button.add_css_class("flat");
        skip_back_button.set_tooltip_text(Some("Skip Back 10s"));

        let play_pause_button = Button::from_icon_name("media-playback-start-symbolic");
        play_pause_button.add_css_class("circular");

        let skip_forward_button = Button::from_icon_name("media-seek-forward-symbolic");
        skip_forward_button.add_css_class("flat");
        skip_forward_button.set_tooltip_text(Some("Skip Forward 30s"));

        let next_button = Button::from_icon_name("media-skip-forward-symbolic");
        next_button.add_css_class("flat");
        next_button.set_tooltip_text(Some("Next Episode"));
        next_button.set_sensitive(false);

        let stop_button = Button::from_icon_name("media-playback-stop-symbolic");
        stop_button.add_css_class("flat");
        stop_button.set_tooltip_text(Some("Stop"));

        transport_box.append(&prev_button);
        transport_box.append(&skip_back_button);
        transport_box.append(&play_pause_button);
        transport_box.append(&skip_forward_button);
        transport_box.append(&next_button);
        transport_box.append(&stop_button);

        // -- Right column: volume, PiP, fullscreen --
        let utility_box = GtkBox::new(Orientation::Horizontal, 4);
        utility_box.set_halign(gtk4::Align::End);
        utility_box.set_hexpand(true);
        utility_box.set_valign(gtk4::Align::Center);

        let volume_button = Button::from_icon_name("audio-volume-high-symbolic");
        volume_button.add_css_class("flat");

        let volume_scale = Scale::with_range(Orientation::Horizontal, 0.0, 1.0, 0.05);
        volume_scale.set_value(1.0);
        volume_scale.set_size_request(100, -1);
        volume_scale.set_draw_value(false);

        let pip_button = Button::new();
        pip_button.set_child(Some(&pip_icon_svg()));
        pip_button.add_css_class("flat");
        pip_button.set_tooltip_text(Some("Picture-in-Picture"));

        let (quick_settings_button, quick_settings_popover) = quick_settings::build(pipeline);

        utility_box.append(&volume_button);
        utility_box.append(&volume_scale);
        utility_box.append(&quick_settings_button);
        utility_box.append(&pip_button);

        button_row.append(&meta_box);
        button_row.append(&transport_box);
        button_row.append(&utility_box);
        controls_bar.append(&button_row);

        bottom_revealer.set_child(Some(&controls_bar));
        overlay.add_overlay(&bottom_revealer);

        // --- Skip Intro / Skip Credits button (bottom-right, above controls) ---
        let skip_action_revealer = Revealer::new();
        skip_action_revealer.set_transition_type(RevealerTransitionType::SlideLeft);
        skip_action_revealer.set_transition_duration(250);
        skip_action_revealer.set_reveal_child(false);
        skip_action_revealer.set_halign(gtk4::Align::End);
        skip_action_revealer.set_valign(gtk4::Align::End);
        skip_action_revealer.set_margin_end(16);
        skip_action_revealer.set_margin_bottom(100);

        let skip_action_button = Button::with_label("Skip Intro");
        skip_action_button.add_css_class("osd");
        skip_action_button.add_css_class("pill");
        skip_action_button.set_size_request(120, -1);
        skip_action_revealer.set_child(Some(&skip_action_button));
        overlay.add_overlay(&skip_action_revealer);

        // --- Up Next card (bottom-right, above skip button) ---
        let up_next_revealer = Revealer::new();
        up_next_revealer.set_transition_type(RevealerTransitionType::SlideLeft);
        up_next_revealer.set_transition_duration(300);
        up_next_revealer.set_reveal_child(false);
        up_next_revealer.set_halign(gtk4::Align::End);
        up_next_revealer.set_valign(gtk4::Align::End);
        up_next_revealer.set_margin_end(16);
        up_next_revealer.set_margin_bottom(160);

        let up_next_card = GtkBox::new(Orientation::Horizontal, 10);
        up_next_card.add_css_class("osd");
        up_next_card.set_margin_top(8);
        up_next_card.set_margin_bottom(8);
        up_next_card.set_margin_start(10);
        up_next_card.set_margin_end(10);

        let up_next_thumb = Picture::new();
        up_next_thumb.set_size_request(120, 68);
        up_next_thumb.set_content_fit(gtk4::ContentFit::Cover);
        up_next_card.append(&up_next_thumb);

        let up_next_info = GtkBox::new(Orientation::Vertical, 2);
        up_next_info.set_valign(gtk4::Align::Center);

        let up_next_header = Label::new(Some("Up Next"));
        up_next_header.add_css_class("dim-label");
        up_next_header.add_css_class("caption");
        up_next_header.set_xalign(0.0);
        up_next_info.append(&up_next_header);

        let up_next_title = Label::new(None);
        up_next_title.add_css_class("heading");
        up_next_title.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        up_next_title.set_max_width_chars(25);
        up_next_title.set_xalign(0.0);
        up_next_info.append(&up_next_title);

        let up_next_subtitle = Label::new(None);
        up_next_subtitle.add_css_class("dim-label");
        up_next_subtitle.add_css_class("caption");
        up_next_subtitle.set_xalign(0.0);
        up_next_info.append(&up_next_subtitle);

        let up_next_countdown = Label::new(None);
        up_next_countdown.add_css_class("caption");
        up_next_countdown.set_xalign(0.0);
        up_next_info.append(&up_next_countdown);

        let up_next_buttons = GtkBox::new(Orientation::Horizontal, 4);
        up_next_buttons.set_margin_top(4);
        let up_next_play_button = Button::with_label("Play Now");
        up_next_play_button.add_css_class("suggested-action");
        up_next_play_button.add_css_class("pill");
        let up_next_cancel_button = Button::from_icon_name("window-close-symbolic");
        up_next_cancel_button.add_css_class("flat");
        up_next_cancel_button.set_tooltip_text(Some("Cancel"));
        up_next_buttons.append(&up_next_play_button);
        up_next_buttons.append(&up_next_cancel_button);
        up_next_info.append(&up_next_buttons);

        up_next_card.append(&up_next_info);
        up_next_revealer.set_child(Some(&up_next_card));
        overlay.add_overlay(&up_next_revealer);

        // --- Connect play/pause ---
        let pipe = pipeline.clone();
        let btn = play_pause_button.clone();
        play_pause_button.connect_clicked(move |_| {
            let p = pipe.lock().unwrap();
            p.toggle_play_pause();
            if p.is_playing() {
                btn.set_icon_name("media-playback-pause-symbolic");
            } else {
                btn.set_icon_name("media-playback-start-symbolic");
            }
        });

        // --- Click video canvas to toggle play/pause ---
        // Attach to the underlying Picture so clicking on controls does not
        // trigger a second toggle.
        let pipe_canvas = pipeline.clone();
        let btn_canvas = play_pause_button.clone();
        let canvas_click = gtk4::GestureClick::new();
        canvas_click.set_button(gtk4::gdk::BUTTON_PRIMARY);
        canvas_click.connect_released(move |_, _, _, _| {
            let p = pipe_canvas.lock().unwrap();
            p.toggle_play_pause();
            if p.is_playing() {
                btn_canvas.set_icon_name("media-playback-pause-symbolic");
            } else {
                btn_canvas.set_icon_name("media-playback-start-symbolic");
            }
        });
        {
            let p = pipeline.lock().unwrap();
            p.picture.add_controller(canvas_click);
        }

        // --- Connect seek bar ---
        let updating_seek = Rc::new(Cell::new(false));
        let pipe2 = pipeline.clone();
        let seek_flag = updating_seek.clone();
        seek_bar.connect_value_changed(move |scale| {
            if seek_flag.get() {
                return;
            }
            let p = pipe2.lock().unwrap();
            if let Some(dur) = p.duration() {
                let frac = scale.value() / 100.0;
                p.seek(frac * dur);
            }
        });

        // --- Connect volume ---
        let pipe3 = pipeline.clone();
        volume_scale.connect_value_changed(move |scale| {
            let p = pipe3.lock().unwrap();
            p.set_volume(scale.value());
        });

        // --- Auto-hide overlays ---
        let hide_timer: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));
        let motion_controller = gtk4::EventControllerMotion::new();
        let top_rev = top_revealer.clone();
        let bot_rev = bottom_revealer.clone();
        let timer = hide_timer.clone();
        motion_controller.connect_motion(move |_, _, _| {
            top_rev.set_reveal_child(true);
            bot_rev.set_reveal_child(true);
            if let Some(id) = timer.borrow_mut().take() {
                id.remove();
            }
            let top_hide = top_rev.clone();
            let bot_hide = bot_rev.clone();
            let timer_clear = timer.clone();
            let id = glib::timeout_add_local_once(std::time::Duration::from_secs(3), move || {
                top_hide.set_reveal_child(false);
                bot_hide.set_reveal_child(false);
                timer_clear.borrow_mut().take();
            });
            *timer.borrow_mut() = Some(id);
        });
        overlay.add_controller(motion_controller);

        let window_handle = WindowHandle::new();
        window_handle.set_vexpand(true);
        window_handle.set_hexpand(true);
        window_handle.set_child(Some(&overlay));

        Self {
            widget: window_handle,
            controls_bar,
            play_pause_button,
            seek_bar,
            volume_scale,
            pip_button,
            back_button,
            fullscreen_button,
            title_label,
            position_label,
            duration_label,
            show_label,
            episode_label,
            prev_button,
            next_button,
            stop_button,
            skip_back_button,
            skip_forward_button,
            quick_settings_button,
            quick_settings_popover,
            updating_seek,
            skip_action_revealer,
            skip_action_button,
            up_next_revealer,
            up_next_title,
            up_next_subtitle,
            up_next_thumb,
            up_next_countdown,
            up_next_play_button,
            up_next_cancel_button,
        }
    }

    /// Update the show/episode metadata labels.
    pub fn set_metadata(&self, show: &str, episode: &str) {
        self.show_label.set_text(show);
        self.episode_label.set_text(episode);
    }

    /// Update position/duration labels and seek bar. Call from a timer.
    pub fn update_position(&self, pipeline: &PlayerPipeline) {
        if let (Some(pos), Some(dur)) = (pipeline.position(), pipeline.duration()) {
            self.position_label.set_text(&format_time(pos));
            self.duration_label.set_text(&format_time(dur));
            if dur > 0.0 {
                self.updating_seek.set(true);
                self.seek_bar.set_value(pos / dur * 100.0);
                self.updating_seek.set(false);
            }
        }
    }

    pub fn show_skip_action(&self, label: &str) {
        self.skip_action_button.set_label(label);
        self.skip_action_revealer.set_reveal_child(true);
    }

    pub fn hide_skip_action(&self) {
        self.skip_action_revealer.set_reveal_child(false);
    }

    pub fn show_up_next(&self, title: &str, subtitle: &str) {
        self.up_next_title.set_text(title);
        self.up_next_subtitle.set_text(subtitle);
        self.up_next_revealer.set_reveal_child(true);
    }

    pub fn hide_up_next(&self) {
        self.up_next_revealer.set_reveal_child(false);
    }

    pub fn set_up_next_countdown(&self, secs: u8) {
        self.up_next_countdown
            .set_text(&format!("Playing in {}...", secs));
    }

    pub fn load_up_next_thumb(&self, url: &str) {
        let thumb = self.up_next_thumb.clone();
        let url = url.to_string();
        let (tx, rx) = async_channel::bounded::<glib::Bytes>(1);
        crate::app::runtime().spawn(async move {
            let Ok(resp) = reqwest::get(&url).await else {
                return;
            };
            let Ok(bytes) = resp.bytes().await else {
                return;
            };
            let _ = tx.send(glib::Bytes::from(&bytes)).await;
        });
        glib::spawn_future_local(async move {
            if let Ok(gbytes) = rx.recv().await {
                if let Ok(texture) = gdk4::Texture::from_bytes(&gbytes) {
                    thumb.set_paintable(Some(&texture));
                }
            }
        });
    }
}
