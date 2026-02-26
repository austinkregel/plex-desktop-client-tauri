use gtk4::prelude::*;
use gtk4::FlowBox;
use std::rc::Rc;

use simplex_core::api::library::MetadataItem;

use super::media_card::{MediaCard, MediaCardStyle};

#[derive(Clone)]
pub struct PosterGrid {
    pub widget: FlowBox,
    style: MediaCardStyle,
}

impl PosterGrid {
    pub fn new() -> Self {
        Self::new_with_style(MediaCardStyle::Poster)
    }

    pub fn new_square() -> Self {
        Self::new_with_style(MediaCardStyle::Square)
    }

    pub fn new_landscape() -> Self {
        Self::new_with_style(MediaCardStyle::Landscape)
    }

    pub fn new_with_style(style: MediaCardStyle) -> Self {
        let flow_box = FlowBox::new();
        flow_box.set_homogeneous(false);
        flow_box.set_min_children_per_line(2);
        flow_box.set_max_children_per_line(10);
        flow_box.set_selection_mode(gtk4::SelectionMode::None);
        flow_box.set_valign(gtk4::Align::Start);
        flow_box.set_column_spacing(8);
        flow_box.set_row_spacing(8);

        Self { widget: flow_box, style }
    }

    /// Add a single card (no click handling).
    pub fn add_entry(&self, title: &str, subtitle: Option<&str>, thumb_url: Option<&str>) {
        let card = MediaCard::new_with_style(title, subtitle, thumb_url, self.style);
        self.widget.append(&card.widget);
    }

    /// Add a single card with a click handler keyed by `rating_key`.
    pub fn add_entry_interactive(
        &self,
        title: &str,
        subtitle: Option<&str>,
        thumb_url: Option<&str>,
        rating_key: &str,
        on_click: &Rc<dyn Fn(&str)>,
    ) {
        let card = MediaCard::new_with_style(title, subtitle, thumb_url, self.style);
        let key = rating_key.to_string();
        let cb = on_click.clone();
        let gesture = gtk4::GestureClick::new();
        gesture.connect_released(move |g, _, _, _| {
            g.set_state(gtk4::EventSequenceState::Claimed);
            cb(&key);
        });
        card.widget.add_controller(gesture);
        card.widget.set_cursor_from_name(Some("pointer"));
        self.widget.append(&card.widget);
    }

    /// Build cards for metadata items (no click handling).
    pub fn add_metadata_items(&self, items: &[MetadataItem], base_url: &str, token: &str) {
        for item in items {
            let thumb = item.best_thumb_url(base_url, token);
            let subtitle = item.display_subtitle();
            self.add_entry(&item.title, subtitle.as_deref(), thumb.as_deref());
        }
    }

    /// Build cards for metadata items with click navigation.
    pub fn add_metadata_items_interactive(
        &self,
        items: &[MetadataItem],
        base_url: &str,
        token: &str,
        on_click: Rc<dyn Fn(&str)>,
    ) {
        for item in items {
            let thumb = item.best_thumb_url(base_url, token);
            let subtitle = item.display_subtitle();
            self.add_entry_interactive(
                &item.title,
                subtitle.as_deref(),
                thumb.as_deref(),
                &item.rating_key,
                &on_click,
            );
        }
    }
}
