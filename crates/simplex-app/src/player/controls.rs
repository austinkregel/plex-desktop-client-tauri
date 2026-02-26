//! Player controls overlay with seek bar, metadata, transport, and utility buttons.

use gtk4::prelude::*;
use gtk4::{
    Box as GtkBox, Button, Label, Orientation, Overlay, Revealer, RevealerTransitionType, Scale,
    WindowHandle,
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use super::pipeline::PlayerPipeline;
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
    updating_seek: Rc<Cell<bool>>,
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

        utility_box.append(&volume_button);
        utility_box.append(&volume_scale);
        utility_box.append(&pip_button);

        button_row.append(&meta_box);
        button_row.append(&transport_box);
        button_row.append(&utility_box);
        controls_bar.append(&button_row);

        bottom_revealer.set_child(Some(&controls_bar));
        overlay.add_overlay(&bottom_revealer);

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
            let id = glib::timeout_add_local_once(
                std::time::Duration::from_secs(3),
                move || {
                    top_hide.set_reveal_child(false);
                    bot_hide.set_reveal_child(false);
                    timer_clear.borrow_mut().take();
                },
            );
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
            updating_seek,
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

}
