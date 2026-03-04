use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Button, Label, Orientation, ScrolledWindow, Spinner};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use crate::widgets::poster_grid::PosterGrid;
use crate::window::AppState;

struct HubData {
    sections: Vec<(String, Vec<simplex_core::api::library::MetadataItem>)>,
    base_url: String,
    token: String,
}

enum HubResult {
    Ok(HubData),
    Err { message: String, is_auth_failure: bool },
}

pub fn build(state: Arc<Mutex<AppState>>) -> GtkBox {
    let container = GtkBox::new(Orientation::Vertical, 8);
    container.set_vexpand(true);

    let spinner = Spinner::new();
    spinner.set_spinning(true);
    spinner.set_halign(gtk4::Align::Center);
    spinner.set_valign(gtk4::Align::Center);
    spinner.set_vexpand(true);
    container.append(&spinner);

    let scroll = ScrolledWindow::new();
    let content = GtkBox::new(Orientation::Vertical, 16);
    content.set_margin_start(16);
    content.set_margin_end(16);
    content.set_margin_top(16);
    content.set_margin_bottom(16);
    scroll.set_child(Some(&content));
    scroll.set_vexpand(true);
    scroll.set_visible(false);
    container.append(&scroll);

    let loaded = Arc::new(Cell::new(false));

    let state_c = state.clone();
    let spinner_c = spinner.clone();
    let scroll_c = scroll.clone();
    let content_c = content.clone();
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
            None => {
                spinner_c.set_visible(false);
                let label = Label::new(Some("Not signed in. Use the login page to authenticate."));
                label.add_css_class("dim-label");
                label.set_halign(gtk4::Align::Center);
                label.set_valign(gtk4::Align::Center);
                label.set_vexpand(true);
                content_c.append(&label);
                scroll_c.set_visible(true);
                return;
            }
        };

        loaded_c.set(true);

        let (tx, rx) = async_channel::unbounded::<HubResult>();

        let base_url_tx = base_url.clone();
        let token_tx = token.clone();
        crate::app::runtime().spawn(async move {
            tracing::info!("Fetching hubs from {}", base_url_tx);
            match simplex_core::api::hubs::get_hubs(&base_url_tx, &token_tx).await {
                Ok(hubs) => {
                    tracing::info!("Got {} hubs", hubs.len());
                    let hubs = simplex_core::api::hubs::deduplicate_hubs(hubs);
                    let sections: Vec<_> = hubs
                        .into_iter()
                        .map(|h| (h.title, h.metadata))
                        .collect();
                    let _ = tx.send(HubResult::Ok(HubData {
                        sections,
                        base_url: base_url_tx,
                        token: token_tx,
                    })).await;
                }
                Err(e) => {
                    let msg = format!("{e}");
                    tracing::error!("Failed to fetch hubs: {}", msg);
                    let is_auth = msg.contains("Unauthorized") || msg.contains("401");
                    let user_msg = if is_auth {
                        "Your Plex session has expired. Please sign in again."
                    } else {
                        "Could not load your Plex home feed. Please check connectivity and try again."
                    };
                    let _ = tx.send(HubResult::Err {
                        message: user_msg.to_string(),
                        is_auth_failure: is_auth,
                    }).await;
                }
            }
        });

        let spinner2 = spinner_c.clone();
        let scroll2 = scroll_c.clone();
        let content2 = content_c.clone();
        let state_click = state_c.clone();
        let loaded2 = loaded_c.clone();
        glib::spawn_future_local(async move {
            if let Ok(result) = rx.recv().await {
                spinner2.set_visible(false);
                scroll2.set_visible(true);

                match result {
                    HubResult::Ok(data) => {
                        if data.sections.is_empty() {
                            let label = Label::new(Some("No content on deck."));
                            label.add_css_class("dim-label");
                            label.set_halign(gtk4::Align::Center);
                            content2.append(&label);
                        }
                        let on_click: Rc<dyn Fn(&str)> = Rc::new(move |key: &str| {
                            crate::window::navigate_to_detail(&state_click, key, "on-deck");
                        });

                        for (title, items) in &data.sections {
                            let section_label = Label::new(Some(title));
                            section_label.set_halign(gtk4::Align::Start);
                            section_label.add_css_class("title-2");
                            content2.append(&section_label);

                            let grid = PosterGrid::new();
                            grid.add_metadata_items_interactive(
                                items, &data.base_url, &data.token, on_click.clone(),
                            );
                            content2.append(&grid.widget);
                        }
                    }
                    HubResult::Err { message, is_auth_failure } => {
                        let err_box = GtkBox::new(Orientation::Vertical, 12);
                        err_box.set_halign(gtk4::Align::Center);
                        err_box.set_valign(gtk4::Align::Center);
                        err_box.set_vexpand(true);

                        let label = Label::new(Some(&message));
                        label.add_css_class("dim-label");
                        label.set_halign(gtk4::Align::Center);
                        label.set_wrap(true);
                        err_box.append(&label);

                        if is_auth_failure {
                            let _ = simplex_core::keychain::clear_auth_token();
                            let mut s = state_click.lock().unwrap();
                            s.token = None;
                            drop(s);

                            let sign_in_btn = Button::with_label("Sign In");
                            sign_in_btn.add_css_class("suggested-action");
                            sign_in_btn.add_css_class("pill");
                            let state_login = state_click.clone();
                            let loaded_login = loaded2.clone();
                            sign_in_btn.connect_clicked(move |_| {
                                loaded_login.set(false);
                                if let Some(vs) = state_login.lock().unwrap().view_stack.clone() {
                                    vs.set_visible_child_name("login");
                                }
                            });
                            err_box.append(&sign_in_btn);
                        }

                        content2.append(&err_box);
                    }
                }
            }
        });
    });

    container
}

