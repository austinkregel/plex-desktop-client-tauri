use crate::window::AppState;
use gtk4::prelude::*;
use gtk4::{
    Box as GtkBox, Button, CheckButton, Label, ListBox, ListBoxRow, Orientation, Popover,
    ScrolledWindow, SelectionMode, Separator,
};
use libadwaita::ViewStack;
use simplex_core::api::library::LibrarySection;
use simplex_core::config;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

pub fn build(view_stack: &ViewStack, state: Arc<Mutex<AppState>>) -> GtkBox {
    let sidebar_box = GtkBox::new(Orientation::Vertical, 0);

    let header = libadwaita::HeaderBar::new();
    header.set_show_end_title_buttons(false);
    sidebar_box.append(&header);

    let listbox = ListBox::new();
    listbox.set_selection_mode(SelectionMode::Single);
    listbox.add_css_class("navigation-sidebar");

    let pinned_from_config = config::load_config().pinned_library_keys;
    let pinned_keys = Rc::new(RefCell::new(pinned_from_config));
    let sections = Rc::new(RefCell::new(Vec::<LibrarySection>::new()));
    let nav_row_ids = Rc::new(RefCell::new(Vec::<String>::new()));

    rebuild_nav_rows(
        &listbox,
        &sections.borrow(),
        &pinned_keys.borrow(),
        &nav_row_ids,
    );

    let stack_ref = view_stack.clone();
    let state_for_nav = state.clone();
    let nav_ids_for_select = nav_row_ids.clone();
    listbox.connect_row_selected(move |_, row| {
        if let Some(row) = row {
            let nav_id = {
                let ids = nav_ids_for_select.borrow();
                let idx = row.index();
                if idx < 0 {
                    return;
                }
                match ids.get(idx as usize) {
                    Some(id) => id.clone(),
                    None => return,
                }
            };

            let mut s = state_for_nav.lock().unwrap();
            let authenticated = s.token.is_some() && s.server.is_some();
            if authenticated {
                if let Some(section_key) = nav_id.strip_prefix("library:") {
                    s.selected_library_key = Some(section_key.to_string());
                    drop(s);
                    let was_library = stack_ref.visible_child_name().as_deref() == Some("library");
                    if was_library {
                        stack_ref.set_visible_child_name("on-deck");
                    }
                    stack_ref.set_visible_child_name("library");
                    return;
                }

                if nav_id == "library" {
                    s.selected_library_key = None;
                    drop(s);
                    let was_library = stack_ref.visible_child_name().as_deref() == Some("library");
                    if was_library {
                        stack_ref.set_visible_child_name("on-deck");
                    }
                    stack_ref.set_visible_child_name("library");
                    return;
                }

                drop(s);
                stack_ref.set_visible_child_name(&nav_id);
            }
        }
    });

    // Load library sections once so pinned keys can be resolved to titles.
    {
        let state_sections = state.clone();
        let list_sections = listbox.clone();
        let sections_store = sections.clone();
        let pinned_store = pinned_keys.clone();
        let nav_ids_store = nav_row_ids.clone();
        let (token, base_url) = {
            let s = state_sections.lock().unwrap();
            match s.token.clone().zip(s.base_url().map(String::from)) {
                Some(pair) => pair,
                None => (String::new(), String::new()),
            }
        };
        if !token.is_empty() && !base_url.is_empty() {
            let (tx, rx) = async_channel::unbounded::<Vec<LibrarySection>>();
            crate::app::runtime().spawn(async move {
                match simplex_core::api::library::get_sections(&base_url, &token).await {
                    Ok(found) => {
                        let _ = tx.send(found).await;
                    }
                    Err(e) => tracing::warn!("Failed to fetch sections for sidebar: {e}"),
                }
            });

            glib::spawn_future_local(async move {
                if let Ok(found) = rx.recv().await {
                    *sections_store.borrow_mut() = found;
                    rebuild_nav_rows(
                        &list_sections,
                        &sections_store.borrow(),
                        &pinned_store.borrow(),
                        &nav_ids_store,
                    );
                }
            });
        }
    }

    let scroll = ScrolledWindow::new();
    scroll.set_child(Some(&listbox));
    scroll.set_vexpand(true);
    sidebar_box.append(&scroll);

    sidebar_box.append(&Separator::new(Orientation::Horizontal));

    // User switcher at the bottom
    let user_box = GtkBox::new(Orientation::Horizontal, 8);
    user_box.set_margin_start(12);
    user_box.set_margin_end(12);
    user_box.set_margin_top(8);
    user_box.set_margin_bottom(8);

    let user_label = Label::new(Some("Switch User"));
    user_label.set_hexpand(true);
    user_label.set_halign(gtk4::Align::Start);
    user_label.add_css_class("dim-label");
    user_box.append(&user_label);

    let switch_button = Button::from_icon_name("system-users-symbolic");
    switch_button.add_css_class("flat");
    switch_button.set_tooltip_text(Some("Switch user"));

    let pin_button = Button::from_icon_name("view-pin-symbolic");
    pin_button.add_css_class("flat");
    pin_button.set_tooltip_text(Some("Choose pinned libraries"));

    let sections_for_pins = sections.clone();
    let pinned_for_pins = pinned_keys.clone();
    let list_for_pins = listbox.clone();
    let nav_ids_for_pins = nav_row_ids.clone();
    pin_button.connect_clicked(move |btn| {
        show_pin_libraries_popover(
            btn,
            sections_for_pins.clone(),
            pinned_for_pins.clone(),
            list_for_pins.clone(),
            nav_ids_for_pins.clone(),
        );
    });

    let state_clone = state.clone();
    switch_button.connect_clicked(move |btn| {
        show_user_switcher(btn, &state_clone);
    });

    user_box.append(&pin_button);
    user_box.append(&switch_button);
    sidebar_box.append(&user_box);

    sidebar_box
}

fn make_nav_row(id: &str, title: &str, indent: i32) -> ListBoxRow {
    let row = ListBoxRow::new();
    let label = Label::new(Some(title));
    label.set_halign(gtk4::Align::Start);
    label.set_margin_start(indent);
    label.set_margin_end(12);
    label.set_margin_top(8);
    label.set_margin_bottom(8);
    row.set_child(Some(&label));
    row.set_widget_name(id);
    row
}

fn rebuild_nav_rows(
    listbox: &ListBox,
    sections: &[LibrarySection],
    pinned: &[String],
    nav_row_ids: &Rc<RefCell<Vec<String>>>,
) {
    while let Some(child) = listbox.first_child() {
        listbox.remove(&child);
    }
    nav_row_ids.borrow_mut().clear();

    listbox.append(&make_nav_row("on-deck", "On Deck", 12));
    nav_row_ids.borrow_mut().push("on-deck".to_string());
    listbox.append(&make_nav_row("library", "Libraries", 12));
    nav_row_ids.borrow_mut().push("library".to_string());

    for key in pinned {
        let title = sections
            .iter()
            .find(|s| s.key == *key)
            .map(|s| s.title.as_str())
            .unwrap_or("Pinned Library");
        let nav_id = format!("library:{key}");
        listbox.append(&make_nav_row(&nav_id, title, 24));
        nav_row_ids.borrow_mut().push(nav_id);
    }

    listbox.append(&make_nav_row("search", "Search", 12));
    nav_row_ids.borrow_mut().push("search".to_string());
    listbox.append(&make_nav_row("playlists", "Playlists", 12));
    nav_row_ids.borrow_mut().push("playlists".to_string());
    listbox.append(&make_nav_row("collections", "Collections", 12));
    nav_row_ids.borrow_mut().push("collections".to_string());
    listbox.append(&make_nav_row("settings", "Settings", 12));
    nav_row_ids.borrow_mut().push("settings".to_string());
}

fn show_pin_libraries_popover(
    button: &Button,
    sections: Rc<RefCell<Vec<LibrarySection>>>,
    pinned_keys: Rc<RefCell<Vec<String>>>,
    listbox: ListBox,
    nav_row_ids: Rc<RefCell<Vec<String>>>,
) {
    let popover = Popover::new();
    let content = GtkBox::new(Orientation::Vertical, 4);
    content.set_margin_start(8);
    content.set_margin_end(8);
    content.set_margin_top(8);
    content.set_margin_bottom(8);

    let sections_now = sections.borrow().clone();
    if sections_now.is_empty() {
        let label = Label::new(Some("No libraries found yet"));
        label.add_css_class("dim-label");
        content.append(&label);
    } else {
        for section in sections_now {
            let toggle = CheckButton::with_label(&section.title);
            toggle.set_active(pinned_keys.borrow().iter().any(|k| k == &section.key));
            let key = section.key.clone();
            let sections_store = sections.clone();
            let pinned_store = pinned_keys.clone();
            let list_store = listbox.clone();
            let nav_ids_store = nav_row_ids.clone();
            toggle.connect_toggled(move |cb| {
                let mut pinned = pinned_store.borrow_mut();
                if cb.is_active() {
                    if !pinned.iter().any(|k| k == &key) {
                        pinned.push(key.clone());
                    }
                } else {
                    pinned.retain(|k| k != &key);
                }

                let mut cfg = config::load_config();
                cfg.pinned_library_keys = pinned.clone();
                if let Err(e) = config::save_config(&cfg) {
                    tracing::warn!("Failed to save pinned libraries: {e}");
                }

                rebuild_nav_rows(
                    &list_store,
                    &sections_store.borrow(),
                    &pinned,
                    &nav_ids_store,
                );
            });
            content.append(&toggle);
        }
    }

    popover.set_child(Some(&content));
    popover.set_parent(button);
    popover.popup();
}

fn show_user_switcher(button: &Button, state: &Arc<Mutex<AppState>>) {
    let s = state.lock().unwrap();
    let token = match &s.token {
        Some(t) => t.clone(),
        None => return,
    };
    drop(s);

    let state_clone = state.clone();
    let (tx, rx) = async_channel::unbounded::<Vec<simplex_core::api::users::HomeUser>>();

    crate::app::runtime().spawn(async move {
        if let Ok(users) = simplex_core::api::users::get_home_users(&token).await {
            let _ = tx.send_blocking(users);
        }
    });

    let popover = gtk4::Popover::new();
    let popover_content = GtkBox::new(Orientation::Vertical, 4);
    popover_content.set_margin_start(8);
    popover_content.set_margin_end(8);
    popover_content.set_margin_top(8);
    popover_content.set_margin_bottom(8);

    let loading = Label::new(Some("Loading users..."));
    loading.add_css_class("dim-label");
    popover_content.append(&loading);
    popover.set_child(Some(&popover_content));
    popover.set_parent(button);
    popover.popup();

    let pop = popover.clone();
    let content = popover_content.clone();
    glib::spawn_future_local(async move {
        if let Ok(users) = rx.recv().await {
            while let Some(child) = content.first_child() {
                content.remove(&child);
            }

            if users.is_empty() {
                let label = Label::new(Some("No other users"));
                label.add_css_class("dim-label");
                content.append(&label);
            } else {
                for user in &users {
                    let btn = Button::with_label(&user.title);
                    btn.add_css_class("flat");
                    let user_id = user.id;
                    let state2 = state_clone.clone();
                    let pop2 = pop.clone();
                    btn.connect_clicked(move |_| {
                        switch_to_user(&state2, user_id);
                        pop2.popdown();
                    });
                    content.append(&btn);
                }
            }
        }
    });
}

fn switch_to_user(state: &Arc<Mutex<AppState>>, user_id: u64) {
    let s = state.lock().unwrap();
    let token = match &s.token {
        Some(t) => t.clone(),
        None => return,
    };
    drop(s);

    let (tx, rx) = async_channel::unbounded::<String>();

    crate::app::runtime().spawn(async move {
        match simplex_core::api::users::switch_user(&token, user_id, None).await {
            Ok(resp) => {
                if let Some(new_token) = resp.auth_token {
                    let _ = simplex_core::keychain::set_auth_token(&new_token);
                    let _ = tx.send_blocking(new_token);
                }
            }
            Err(e) => {
                tracing::error!("Failed to switch user: {}", e);
            }
        }
    });

    let state_clone = state.clone();
    glib::spawn_future_local(async move {
        if let Ok(new_token) = rx.recv().await {
            let mut s = state_clone.lock().unwrap();
            s.token = Some(new_token);
            tracing::info!("Switched to user {}", user_id);
        }
    });
}
