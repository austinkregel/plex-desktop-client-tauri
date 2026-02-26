//! Picture-in-Picture window.
//!
//! Creates a borderless, draggable floating window that shares the video
//! paintable from the main player. The window is undecorated; a WindowHandle
//! wrapper makes the entire surface draggable and a close button appears on
//! mouse hover.

use gdk4::prelude::*;
use gtk4::prelude::*;
use gtk4::{Button, Overlay, Picture, Revealer, RevealerTransitionType, Window, WindowHandle};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use super::pipeline::PlayerPipeline;

fn reverse_pip_icon() -> gtk4::Image {
    let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16">
  <rect x="1" y="1" width="14" height="14" rx="1.5" ry="1.5" fill="none" stroke="white" stroke-width="1.5"/>
  <rect x="8" y="8" width="6" height="5" rx="1" ry="1" fill="white"/>
  <path d="M9 12L12 9L12 12Z" fill="white"/>
</svg>"#;
    let bytes = glib::Bytes::from(svg.as_bytes());
    let texture = gdk4::Texture::from_bytes(&bytes).ok();
    let image = gtk4::Image::new();
    if let Some(tex) = texture {
        image.set_paintable(Some(&tex));
    } else {
        image.set_icon_name(Some("view-restore-symbolic"));
    }
    image
}

pub struct PipWindow {
    pub window: Window,
    on_close: Arc<Mutex<Option<Box<dyn Fn() + 'static>>>>,
    on_return: Arc<Mutex<Option<Box<dyn Fn() + 'static>>>>,
}

impl PipWindow {
    /// Create a PiP window sharing the same paintable as the main player.
    pub fn new(pipeline: &Arc<Mutex<PlayerPipeline>>) -> Self {
        let window = Window::new();
        window.set_title(Some("Simplex - PiP"));
        window.set_default_size(480, 270);
        window.set_decorated(false);
        window.set_resizable(true);

        let paintable = {
            let p = pipeline.lock().unwrap();
            p.paintable_sink().property::<gdk4::Paintable>("paintable")
        };
        let picture = Picture::new();
        picture.set_paintable(Some(&paintable));
        picture.set_content_fit(gtk4::ContentFit::Contain);
        picture.set_can_shrink(true);
        picture.set_vexpand(true);
        picture.set_hexpand(true);

        let overlay = Overlay::new();
        overlay.set_child(Some(&picture));

        // Close button (top-right, appears on hover)
        let close_revealer = Revealer::new();
        close_revealer.set_transition_type(RevealerTransitionType::Crossfade);
        close_revealer.set_reveal_child(false);
        close_revealer.set_valign(gtk4::Align::Start);
        close_revealer.set_halign(gtk4::Align::End);

        let close_btn = Button::from_icon_name("window-close-symbolic");
        close_btn.add_css_class("circular");
        close_btn.add_css_class("osd");
        close_btn.set_margin_top(6);
        close_btn.set_margin_end(6);

        let win_close = window.clone();
        close_btn.connect_clicked(move |_| {
            win_close.close();
        });

        close_revealer.set_child(Some(&close_btn));
        overlay.add_overlay(&close_revealer);

        // Return-to-player button (top-left, appears on hover)
        let return_revealer = Revealer::new();
        return_revealer.set_transition_type(RevealerTransitionType::Crossfade);
        return_revealer.set_reveal_child(false);
        return_revealer.set_valign(gtk4::Align::Start);
        return_revealer.set_halign(gtk4::Align::Start);

        let return_btn = Button::new();
        return_btn.set_child(Some(&reverse_pip_icon()));
        return_btn.add_css_class("circular");
        return_btn.add_css_class("osd");
        return_btn.set_tooltip_text(Some("Return to Player"));
        return_btn.set_margin_top(6);
        return_btn.set_margin_start(6);

        let on_return: Arc<Mutex<Option<Box<dyn Fn() + 'static>>>> =
            Arc::new(Mutex::new(None));

        let win_return = window.clone();
        let cb_return = on_return.clone();
        return_btn.connect_clicked(move |_| {
            if let Some(ref f) = *cb_return.lock().unwrap() {
                f();
            }
            win_return.close();
        });

        return_revealer.set_child(Some(&return_btn));
        overlay.add_overlay(&return_revealer);

        // Show both buttons on hover, hide on leave
        let motion = gtk4::EventControllerMotion::new();
        let rev_close_enter = close_revealer.clone();
        let rev_return_enter = return_revealer.clone();
        motion.connect_enter(move |_, _, _| {
            rev_close_enter.set_reveal_child(true);
            rev_return_enter.set_reveal_child(true);
        });
        let rev_close_leave = close_revealer.clone();
        let rev_return_leave = return_revealer.clone();
        motion.connect_leave(move |_| {
            rev_close_leave.set_reveal_child(false);
            rev_return_leave.set_reveal_child(false);
        });
        overlay.add_controller(motion);

        // WindowHandle makes the entire PiP surface draggable
        let handle = WindowHandle::new();
        handle.set_child(Some(&overlay));
        window.set_child(Some(&handle));

        let on_close: Arc<Mutex<Option<Box<dyn Fn() + 'static>>>> =
            Arc::new(Mutex::new(None));

        let cb = on_close.clone();
        window.connect_close_request(move |_| {
            if let Some(ref f) = *cb.lock().unwrap() {
                f();
            }
            glib::Propagation::Proceed
        });

        // Maintain 16:9 aspect ratio on resize by adjusting height when
        // the width changes.
        let aspect = Rc::new(RefCell::new(16.0_f64 / 9.0));
        let win_resize = window.clone();
        let ratio = aspect.clone();
        window.connect_default_width_notify(move |_| {
            let w = win_resize.default_width();
            if w > 0 {
                let r = *ratio.borrow();
                let h = (w as f64 / r).round() as i32;
                win_resize.set_default_size(w, h);
            }
        });

        Self { window, on_close, on_return }
    }

    /// Register a callback invoked when the PiP window is closed.
    pub fn on_close<F: Fn() + 'static>(&self, f: F) {
        *self.on_close.lock().unwrap() = Some(Box::new(f));
    }

    /// Register a callback invoked when the user clicks the return-to-player button.
    pub fn on_return<F: Fn() + 'static>(&self, f: F) {
        *self.on_return.lock().unwrap() = Some(Box::new(f));
    }

    pub fn show(&self) {
        self.window.present();
    }

    pub fn hide(&self) {
        self.window.close();
    }

    pub fn is_visible(&self) -> bool {
        self.window.is_visible()
    }
}
