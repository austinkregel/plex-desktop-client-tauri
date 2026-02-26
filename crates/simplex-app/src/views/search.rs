use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Label, Orientation, ScrolledWindow, SearchEntry, Spinner};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use crate::widgets::poster_grid::PosterGrid;
use crate::window::AppState;

pub fn build(state: Arc<Mutex<AppState>>) -> GtkBox {
    let container = GtkBox::new(Orientation::Vertical, 8);
    container.set_margin_start(16);
    container.set_margin_end(16);
    container.set_margin_top(16);
    container.set_vexpand(true);

    let search_entry = SearchEntry::new();
    search_entry.set_placeholder_text(Some("Search your library..."));
    container.append(&search_entry);

    let results_scroll = ScrolledWindow::new();
    results_scroll.set_vexpand(true);
    let results_area = GtkBox::new(Orientation::Vertical, 0);
    results_scroll.set_child(Some(&results_area));
    container.append(&results_scroll);

    let state_clone = state.clone();
    let results_clone = results_area.clone();
    search_entry.connect_search_changed(move |entry| {
        let query = entry.text().to_string();
        if query.len() < 2 {
            return;
        }

        while let Some(child) = results_clone.first_child() {
            results_clone.remove(&child);
        }

        let spinner = Spinner::new();
        spinner.set_spinning(true);
        spinner.set_halign(gtk4::Align::Center);
        results_clone.append(&spinner);

        let s = state_clone.lock().unwrap();
        let token_url = s.token.clone().zip(s.base_url().map(String::from));
        drop(s);

        if let Some((token, base_url)) = token_url {
            let (tx, rx) = async_channel::unbounded::<(Vec<simplex_core::api::library::MetadataItem>, String, String)>();
            let bu = base_url.clone();
            let tk = token.clone();
            crate::app::runtime().spawn(async move {
                if let Ok(items) =
                    simplex_core::api::search::search(&bu, &tk, &query).await
                {
                    let _ = tx.send((items, bu, tk)).await;
                }
            });

            let results = results_clone.clone();
            let spin = spinner.clone();
            let state_click = state_clone.clone();
            glib::spawn_future_local(async move {
                if let Ok((items, base_url, token)) = rx.recv().await {
                    spin.set_visible(false);
                    if items.is_empty() {
                        let label = Label::new(Some("No results found"));
                        label.add_css_class("dim-label");
                        results.append(&label);
                    } else {
                        let on_click: Rc<dyn Fn(&str)> = Rc::new(move |key: &str| {
                            crate::window::navigate_to_detail(&state_click, key, "search");
                        });
                        let grid = PosterGrid::new();
                        grid.add_metadata_items_interactive(
                            &items, &base_url, &token, on_click,
                        );
                        results.append(&grid.widget);
                    }
                }
            });
        }
    });

    container
}
