use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Label, Orientation, ScrolledWindow, Spinner};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use crate::widgets::poster_grid::PosterGrid;
use crate::window::AppState;

pub fn build(state: Arc<Mutex<AppState>>) -> GtkBox {
    let container = GtkBox::new(Orientation::Vertical, 8);
    container.set_vexpand(true);
    container.set_margin_start(16);
    container.set_margin_end(16);
    container.set_margin_top(16);

    let title = Label::new(Some("Playlists"));
    title.add_css_class("title-2");
    title.set_halign(gtk4::Align::Start);
    container.append(&title);

    let spinner = Spinner::new();
    spinner.set_spinning(true);
    spinner.set_halign(gtk4::Align::Center);
    spinner.set_vexpand(true);
    container.append(&spinner);

    let content_scroll = ScrolledWindow::new();
    content_scroll.set_vexpand(true);
    content_scroll.set_visible(false);
    let content_box = GtkBox::new(Orientation::Vertical, 0);
    content_scroll.set_child(Some(&content_box));
    container.append(&content_scroll);

    let loaded = Arc::new(Cell::new(false));

    let state_c = state.clone();
    let spinner_c = spinner.clone();
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

        let (tx, rx) = async_channel::unbounded::<(
            Vec<simplex_core::api::playlists::Playlist>,
            String,
            String,
        )>();
        let bu = base_url.clone();
        let tk = token.clone();
        crate::app::runtime().spawn(async move {
            if let Ok(playlists) = simplex_core::api::playlists::get_playlists(&bu, &tk).await {
                let _ = tx.send((playlists, bu, tk)).await;
            }
        });

        let spin2 = spinner_c.clone();
        let scroll2 = content_scroll.clone();
        let content2 = content_box.clone();
        let state_click = state_c.clone();
        glib::spawn_future_local(async move {
            if let Ok((playlists, base_url, token)) = rx.recv().await {
                spin2.set_visible(false);
                scroll2.set_visible(true);
                let on_click: Rc<dyn Fn(&str)> = Rc::new(move |key: &str| {
                    crate::window::navigate_to_detail(&state_click, key, "playlists");
                });
                let grid = PosterGrid::new();
                for pl in &playlists {
                    let thumb = pl
                        .thumb
                        .as_deref()
                        .map(|t| simplex_core::api::library::thumb_url(&base_url, &token, t));
                    let subtitle = pl.leaf_count.map(|c| format!("{} items", c));
                    grid.add_entry_interactive(
                        &pl.title,
                        subtitle.as_deref(),
                        thumb.as_deref(),
                        &pl.rating_key,
                        &on_click,
                    );
                }
                content2.append(&grid.widget);
            }
        });
    });

    container
}
