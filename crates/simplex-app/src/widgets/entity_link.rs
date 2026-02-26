use gtk4::prelude::*;
use gtk4::Button;
use std::sync::{Arc, Mutex};

use crate::window::AppState;

/// Creates a flat button that behaves like a clickable entity name.
pub fn make_entity_link(
    text: &str,
    rating_key: &str,
    from_view: &'static str,
    state: Arc<Mutex<AppState>>,
) -> Button {
    let btn = Button::with_label(text);
    btn.add_css_class("flat");
    btn.set_halign(gtk4::Align::Start);
    btn.set_can_focus(false);
    btn.set_cursor_from_name(Some("pointer"));

    let key = rating_key.to_string();
    btn.connect_clicked(move |_| {
        crate::window::navigate_to_detail(&state, &key, from_view);
    });

    btn
}
