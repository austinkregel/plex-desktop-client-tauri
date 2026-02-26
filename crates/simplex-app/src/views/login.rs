use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Button, Label, Orientation, Spinner, Align};
use libadwaita::prelude::*;
use std::sync::{Arc, Mutex};

use crate::window::AppState;

pub fn build(state: Arc<Mutex<AppState>>) -> GtkBox {
    let container = GtkBox::new(Orientation::Vertical, 16);
    container.set_halign(Align::Center);
    container.set_valign(Align::Center);
    container.set_margin_start(32);
    container.set_margin_end(32);

    let title = Label::new(Some("Welcome to Simplex"));
    title.add_css_class("title-1");
    container.append(&title);

    let subtitle = Label::new(Some("Sign in with your Plex account to get started"));
    subtitle.add_css_class("dim-label");
    container.append(&subtitle);

    let spinner = Spinner::new();
    spinner.set_visible(false);
    container.append(&spinner);

    let status_label = Label::new(None);
    status_label.set_visible(false);
    container.append(&status_label);

    let button = Button::with_label("Sign in with Plex");
    button.add_css_class("suggested-action");
    button.add_css_class("pill");

    let state_clone = state.clone();
    let spinner_clone = spinner.clone();
    let status_clone = status_label.clone();
    let button_clone = button.clone();

    button.connect_clicked(move |_| {
        let state = state_clone.clone();
        let spinner = spinner_clone.clone();
        let status = status_clone.clone();
        let btn = button_clone.clone();

        btn.set_sensitive(false);
        spinner.set_visible(true);
        spinner.set_spinning(true);
        status.set_visible(true);
        status.set_text("Opening browser for authentication...");

        let client_id = {
            let s = state.lock().unwrap();
            s.client_id.clone()
        };

        let (tx, rx) = async_channel::unbounded();

        crate::app::runtime().spawn(async move {
            match simplex_core::api::auth::create_pin(&client_id).await {
                Ok(pin) => {
                    let auth_url =
                        simplex_core::api::auth::get_auth_url(&pin.code, &client_id);
                    let _ = open::that(&auth_url);
                    let _ = tx.send_blocking("Waiting for authentication...".to_string());

                    // Poll for token
                    for _ in 0..150 {
                        // 5 minutes max
                        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                        match simplex_core::api::auth::check_pin(pin.id, &client_id).await {
                            Ok(checked) => {
                                if let Some(token) = checked.auth_token {
                                    match simplex_core::discovery::discover_servers_from_token(
                                        token,
                                    )
                                    .await
                                    {
                                        Ok(_) => {
                                            let _ = tx.send_blocking("AUTH_SUCCESS".to_string());
                                            return;
                                        }
                                        Err(e) => {
                                            let _ = tx.send_blocking(format!("ERROR:{}", e));
                                            return;
                                        }
                                    }
                                }
                            }
                            Err(_) => {}
                        }
                    }
                    let _ = tx.send_blocking("AUTH_TIMEOUT".to_string());
                }
                Err(e) => {
                    let _ = tx.send_blocking(format!("ERROR:{}", e));
                }
            }
        });

        let state2 = state.clone();
        let spinner2 = spinner.clone();
        let status2 = status.clone();
        let btn2 = btn.clone();
        glib::spawn_future_local(async move {
            while let Ok(msg) = rx.recv().await {
                if msg == "AUTH_SUCCESS" {
                    status2.set_text("Authentication successful!");
                    spinner2.set_spinning(false);
                    spinner2.set_visible(false);
                    btn2.set_label("Signed In");
                    let view_stack = {
                        let mut s = state2.lock().unwrap();
                        s.token = simplex_core::keychain::get_auth_token().ok().flatten();
                        s.server = simplex_core::config::get_default_server().ok().flatten();
                        s.view_stack.clone()
                    };
                    // Navigate AFTER releasing the lock -- set_visible_child_name
                    // fires connect_map synchronously, which also locks AppState.
                    if let Some(vs) = view_stack {
                        vs.set_visible_child_name("on-deck");
                    }
                } else if msg == "AUTH_TIMEOUT" {
                    status2.set_text("Authentication timed out. Please try again.");
                    spinner2.set_spinning(false);
                    spinner2.set_visible(false);
                    btn2.set_sensitive(true);
                } else if msg.starts_with("ERROR:") {
                    status2.set_text(&msg[6..]);
                    spinner2.set_spinning(false);
                    btn2.set_sensitive(true);
                } else {
                    status2.set_text(&msg);
                }
            }
        });
    });

    container.append(&button);
    container
}
