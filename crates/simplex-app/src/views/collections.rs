use gtk4::prelude::*;
use gtk4::{
    Box as GtkBox, Label, ListBox, ListBoxRow, Orientation, ScrolledWindow, SelectionMode, Spinner,
};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use crate::widgets::poster_grid::PosterGrid;
use crate::window::AppState;

pub fn build(state: Arc<Mutex<AppState>>) -> GtkBox {
    let container = GtkBox::new(Orientation::Vertical, 0);
    container.set_vexpand(true);

    let section_list = ListBox::new();
    section_list.set_selection_mode(SelectionMode::Single);
    section_list.set_margin_start(8);
    section_list.set_margin_end(8);
    section_list.set_margin_top(8);

    let grid_scroll = ScrolledWindow::new();
    grid_scroll.set_vexpand(true);

    let grid_area = GtkBox::new(Orientation::Vertical, 0);
    grid_scroll.set_child(Some(&grid_area));

    let spinner = Spinner::new();
    spinner.set_spinning(true);
    spinner.set_halign(gtk4::Align::Center);
    spinner.set_valign(gtk4::Align::Center);
    spinner.set_vexpand(true);
    grid_area.append(&spinner);

    container.append(&section_list);
    container.append(&grid_scroll);

    let loaded = Arc::new(Cell::new(false));

    let state_c = state.clone();
    let spinner_c = spinner.clone();
    let section_list_c = section_list.clone();
    let grid_c = grid_area.clone();
    let loaded_c = loaded.clone();

    container.connect_map(move |_| {
        if loaded_c.get() {
            return;
        }

        let s = state_c.lock().unwrap();
        let token_url = s.token.clone().zip(s.base_url().map(String::from));
        drop(s);

        let (token, base_url) = match token_url {
            Some(pair) => pair,
            None => return,
        };

        loaded_c.set(true);

        let (tx, rx) =
            async_channel::unbounded::<Vec<simplex_core::api::library::LibrarySection>>();

        crate::app::runtime().spawn(async move {
            if let Ok(sections) = simplex_core::api::library::get_sections(&base_url, &token).await
            {
                let _ = tx.send_blocking(sections);
            }
        });

        let state2 = state_c.clone();
        let spinner2 = spinner_c.clone();
        let list2 = section_list_c.clone();
        let grid2 = grid_c.clone();
        glib::spawn_future_local(async move {
            if let Ok(sections) = rx.recv().await {
                spinner2.set_visible(false);
                for section in &sections {
                    let row = ListBoxRow::new();
                    let label = Label::new(Some(&section.title));
                    label.set_halign(gtk4::Align::Start);
                    label.set_margin_start(8);
                    label.set_margin_end(8);
                    label.set_margin_top(6);
                    label.set_margin_bottom(6);
                    row.set_child(Some(&label));
                    row.set_widget_name(&section.key);
                    list2.append(&row);
                }

                let state3 = state2.clone();
                let grid3 = grid2.clone();
                list2.connect_row_selected(move |_, row| {
                    if let Some(row) = row {
                        let key = row.widget_name().to_string();
                        load_collections(&state3, &grid3, &key);
                    }
                });

                if let Some(first) = list2.row_at_index(0) {
                    list2.select_row(Some(&first));
                }
            }
        });
    });

    container
}

fn load_collections(state: &Arc<Mutex<AppState>>, grid_area: &GtkBox, section_key: &str) {
    while let Some(child) = grid_area.first_child() {
        grid_area.remove(&child);
    }

    let spinner = Spinner::new();
    spinner.set_spinning(true);
    spinner.set_halign(gtk4::Align::Center);
    spinner.set_valign(gtk4::Align::Center);
    spinner.set_vexpand(true);
    grid_area.append(&spinner);

    let s = state.lock().unwrap();
    let token_url = s.token.clone().zip(s.base_url().map(String::from));
    drop(s);

    if let Some((token, base_url)) = token_url {
        let key = section_key.to_string();
        let (tx, rx) = async_channel::unbounded::<(
            Vec<simplex_core::api::library::MetadataItem>,
            String,
            String,
        )>();

        let bu = base_url.clone();
        let tk = token.clone();
        crate::app::runtime().spawn(async move {
            if let Ok(items) = simplex_core::api::library::get_collections(&bu, &tk, &key).await {
                let _ = tx.send((items, bu, tk)).await;
            }
        });

        let grid = grid_area.clone();
        let spin = spinner.clone();
        let state_click = state.clone();
        glib::spawn_future_local(async move {
            if let Ok((items, base_url, token)) = rx.recv().await {
                spin.set_visible(false);
                if items.is_empty() {
                    let label = Label::new(Some("No collections in this library"));
                    label.add_css_class("dim-label");
                    label.set_halign(gtk4::Align::Center);
                    label.set_margin_top(16);
                    grid.append(&label);
                } else {
                    let on_click: Rc<dyn Fn(&str)> = Rc::new(move |key: &str| {
                        crate::window::navigate_to_detail(&state_click, key, "collections");
                    });
                    let poster_grid = PosterGrid::new();
                    poster_grid.add_metadata_items_interactive(&items, &base_url, &token, on_click);
                    grid.append(&poster_grid.widget);
                }
            }
        });
    }
}
