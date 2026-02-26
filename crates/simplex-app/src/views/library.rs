use gtk4::prelude::*;
use gtk4::{
    Box as GtkBox, CheckButton, ComboBoxText, Label, ListBox, ListBoxRow, Orientation,
    ScrolledWindow, SelectionMode, Spinner,
};
use simplex_core::api::library::{FilterOption, LibraryFilter, LibrarySection};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use crate::widgets::poster_grid::PosterGrid;
use crate::window::AppState;

#[derive(Clone, Default)]
struct SectionFilterOptions {
    genres: Vec<FilterOption>,
    years: Vec<FilterOption>,
    content_ratings: Vec<FilterOption>,
    resolutions: Vec<FilterOption>,
    audio_languages: Vec<FilterOption>,
}

#[derive(Clone)]
struct FilterControls {
    sort: ComboBoxText,
    genre: ComboBoxText,
    year: ComboBoxText,
    content_rating: ComboBoxText,
    resolution: ComboBoxText,
    audio_language: ComboBoxText,
    unwatched_only: CheckButton,
}

impl FilterControls {
    fn current_filter(&self) -> LibraryFilter {
        LibraryFilter {
            genre: self.genre.active_id().map(|s| s.to_string()).filter(|s| !s.is_empty()),
            year: self.year.active_id().map(|s| s.to_string()).filter(|s| !s.is_empty()),
            content_rating: self
                .content_rating
                .active_id()
                .map(|s| s.to_string())
                .filter(|s| !s.is_empty()),
            resolution: self
                .resolution
                .active_id()
                .map(|s| s.to_string())
                .filter(|s| !s.is_empty()),
            unwatched_only: self.unwatched_only.is_active(),
            audio_language: self
                .audio_language
                .active_id()
                .map(|s| s.to_string())
                .filter(|s| !s.is_empty()),
            sort: self.sort.active_id().map(|s| s.to_string()).filter(|s| !s.is_empty()),
        }
    }
}

pub fn build(state: Arc<Mutex<AppState>>) -> GtkBox {
    let container = GtkBox::new(Orientation::Vertical, 8);
    container.set_vexpand(true);
    container.set_margin_top(8);
    container.set_margin_start(8);
    container.set_margin_end(8);

    let section_list = ListBox::new();
    section_list.set_selection_mode(SelectionMode::Single);

    let filter_row = GtkBox::new(Orientation::Horizontal, 8);
    filter_row.set_margin_top(4);
    filter_row.set_margin_bottom(4);

    let sort = ComboBoxText::new();
    sort.append(Some("titleSort:asc"), "Title (A-Z)");
    sort.append(Some("addedAt:desc"), "Date Added (Newest)");
    sort.append(Some("rating:desc"), "Rating (Highest)");
    sort.append(Some("year:desc"), "Year (Newest)");
    sort.append(Some("originallyAvailableAt:desc"), "Release Date");
    sort.set_active_id(Some("titleSort:asc"));

    let genre = ComboBoxText::new();
    let year = ComboBoxText::new();
    let content_rating = ComboBoxText::new();
    let resolution = ComboBoxText::new();
    let audio_language = ComboBoxText::new();
    let unwatched_only = CheckButton::with_label("Unwatched only");

    let controls = FilterControls {
        sort: sort.clone(),
        genre: genre.clone(),
        year: year.clone(),
        content_rating: content_rating.clone(),
        resolution: resolution.clone(),
        audio_language: audio_language.clone(),
        unwatched_only: unwatched_only.clone(),
    };

    let mut controls_with_labels: Vec<(&str, &ComboBoxText)> = vec![
        ("Sort", &sort),
        ("Genre", &genre),
        ("Year", &year),
        ("Content Rating", &content_rating),
        ("Resolution", &resolution),
        ("Audio Language", &audio_language),
    ];
    for (label, combo) in controls_with_labels.drain(..) {
        let box_col = GtkBox::new(Orientation::Vertical, 2);
        let lbl = Label::new(Some(label));
        lbl.add_css_class("dim-label");
        lbl.set_halign(gtk4::Align::Start);
        box_col.append(&lbl);
        box_col.append(combo);
        filter_row.append(&box_col);
    }
    filter_row.append(&unwatched_only);

    let filter_scroll = ScrolledWindow::new();
    filter_scroll.set_policy(gtk4::PolicyType::Automatic, gtk4::PolicyType::Never);
    filter_scroll.set_child(Some(&filter_row));

    let grid_scroll = ScrolledWindow::new();
    grid_scroll.set_vexpand(true);
    let grid_area = GtkBox::new(Orientation::Vertical, 0);
    grid_scroll.set_child(Some(&grid_area));

    container.append(&section_list);
    container.append(&filter_scroll);
    container.append(&grid_scroll);

    let sections = Rc::new(RefCell::new(Vec::<LibrarySection>::new()));
    let loaded = Rc::new(Cell::new(false));
    let current_section_key = Rc::new(RefCell::new(None::<String>));
    let filter_cache = Rc::new(RefCell::new(HashMap::<String, SectionFilterOptions>::new()));
    let suppress_filter_events = Rc::new(Cell::new(false));

    populate_filter_combo(&genre, "All Genres", &[]);
    populate_filter_combo(&year, "All Years", &[]);
    populate_filter_combo(&content_rating, "All Ratings", &[]);
    populate_filter_combo(&resolution, "All Resolutions", &[]);
    populate_filter_combo(&audio_language, "All Audio Languages", &[]);

    // Load items whenever the selected section changes.
    {
        let state_sel = state.clone();
        let controls_sel = controls.clone();
        let grid_sel = grid_area.clone();
        let current_sel = current_section_key.clone();
        let sections_sel = sections.clone();
        section_list.connect_row_selected(move |_, row| {
            if let Some(row) = row {
                let key = row.widget_name().to_string();
                *current_sel.borrow_mut() = Some(key.clone());
                let filter = controls_sel.current_filter();
                let section_type = sections_sel
                    .borrow()
                    .iter()
                    .find(|s| s.key == key)
                    .map(|s| s.section_type.clone());
                load_section_items(
                    &state_sel,
                    &grid_sel,
                    &key,
                    &filter,
                    section_type.as_deref(),
                );
            }
        });
    }

    // Shared callback used by all filter controls.
    let on_filter_change: Rc<dyn Fn()> = Rc::new({
        let state_filter = state.clone();
        let controls_filter = controls.clone();
        let grid_filter = grid_area.clone();
        let current_filter = current_section_key.clone();
        let suppress = suppress_filter_events.clone();
        let sections_filter = sections.clone();
        move || {
            if suppress.get() {
                return;
            }
            if let Some(section_key) = current_filter.borrow().clone() {
                let section_type = sections_filter
                    .borrow()
                    .iter()
                    .find(|s| s.key == section_key)
                    .map(|s| s.section_type.clone());
                load_section_items(
                    &state_filter,
                    &grid_filter,
                    &section_key,
                    &controls_filter.current_filter(),
                    section_type.as_deref(),
                );
            }
        }
    });

    {
        let on_change = on_filter_change.clone();
        sort.connect_changed(move |_| on_change());
    }
    {
        let on_change = on_filter_change.clone();
        genre.connect_changed(move |_| on_change());
    }
    {
        let on_change = on_filter_change.clone();
        year.connect_changed(move |_| on_change());
    }
    {
        let on_change = on_filter_change.clone();
        content_rating.connect_changed(move |_| on_change());
    }
    {
        let on_change = on_filter_change.clone();
        resolution.connect_changed(move |_| on_change());
    }
    {
        let on_change = on_filter_change.clone();
        audio_language.connect_changed(move |_| on_change());
    }
    {
        let on_change = on_filter_change;
        unwatched_only.connect_toggled(move |_| on_change());
    }

    // Load sections once; on every map, apply selected section from AppState.
    {
        let state_map = state.clone();
        let section_list_map = section_list.clone();
        let sections_map = sections.clone();
        let loaded_map = loaded.clone();
        let controls_map = controls.clone();
        let cache_map = filter_cache.clone();
        let suppress_map = suppress_filter_events.clone();
        container.connect_map(move |_| {
            apply_sidebar_selection(&state_map, &section_list_map);

            if loaded_map.get() {
                if let Some(row) = section_list_map.selected_row() {
                    let key = row.widget_name().to_string();
                    if let Some(opts) = cache_map.borrow().get(&key) {
                        apply_filter_options_to_controls(&controls_map, opts, &suppress_map);
                    }
                }
                return;
            }

            let (token, base_url) = {
                let s = state_map.lock().unwrap();
                match s.token.clone().zip(s.base_url().map(String::from)) {
                    Some(pair) => pair,
                    None => return,
                }
            };

            loaded_map.set(true);

            let (tx, rx) = async_channel::unbounded::<Vec<LibrarySection>>();
            crate::app::runtime().spawn(async move {
                match simplex_core::api::library::get_sections(&base_url, &token).await {
                    Ok(found) => {
                        let _ = tx.send(found).await;
                    }
                    Err(e) => tracing::warn!("Failed to load library sections: {e}"),
                }
            });

            let state_after = state_map.clone();
            let list_after = section_list_map.clone();
            let sections_after = sections_map.clone();
            glib::spawn_future_local(async move {
                if let Ok(found) = rx.recv().await {
                    *sections_after.borrow_mut() = found.clone();

                    while let Some(child) = list_after.first_child() {
                        list_after.remove(&child);
                    }
                    for section in &found {
                        let row = ListBoxRow::new();
                        let label = Label::new(Some(&section.title));
                        label.set_halign(gtk4::Align::Start);
                        label.set_margin_start(8);
                        label.set_margin_end(8);
                        label.set_margin_top(6);
                        label.set_margin_bottom(6);
                        row.set_child(Some(&label));
                        row.set_widget_name(&section.key);
                        list_after.append(&row);
                    }

                    apply_sidebar_selection(&state_after, &list_after);
                    if list_after.selected_row().is_none() {
                        if let Some(first) = list_after.row_at_index(0) {
                            list_after.select_row(Some(&first));
                        }
                    }
                }
            });
        });
    }

    // Fetch section-specific filter options when section selection changes.
    {
        let state_opts = state.clone();
        let controls_opts = controls.clone();
        let cache_opts = filter_cache.clone();
        let suppress_opts = suppress_filter_events.clone();
        section_list.connect_row_selected(move |_, row| {
            let Some(row) = row else { return };
            let section_key = row.widget_name().to_string();

            if let Some(options) = cache_opts.borrow().get(&section_key).cloned() {
                apply_filter_options_to_controls(&controls_opts, &options, &suppress_opts);
                return;
            }

            let (token, base_url) = {
                let s = state_opts.lock().unwrap();
                match s.token.clone().zip(s.base_url().map(String::from)) {
                    Some(pair) => pair,
                    None => return,
                }
            };
            let section_key_async = section_key.clone();
            let (tx, rx) = async_channel::unbounded::<SectionFilterOptions>();
            crate::app::runtime().spawn(async move {
                let genres = simplex_core::api::library::get_filter_options(
                    &base_url,
                    &token,
                    &section_key_async,
                    "genre",
                )
                .await
                .unwrap_or_default();
                let years = simplex_core::api::library::get_filter_options(
                    &base_url,
                    &token,
                    &section_key_async,
                    "year",
                )
                .await
                .unwrap_or_default();
                let content_ratings = simplex_core::api::library::get_filter_options(
                    &base_url,
                    &token,
                    &section_key_async,
                    "contentRating",
                )
                .await
                .unwrap_or_default();
                let resolutions = simplex_core::api::library::get_filter_options(
                    &base_url,
                    &token,
                    &section_key_async,
                    "resolution",
                )
                .await
                .unwrap_or_default();
                let audio_languages = simplex_core::api::library::get_filter_options(
                    &base_url,
                    &token,
                    &section_key_async,
                    "language",
                )
                .await
                .unwrap_or_default();
                let _ = tx
                    .send(SectionFilterOptions {
                        genres,
                        years,
                        content_ratings,
                        resolutions,
                        audio_languages,
                    })
                    .await;
            });

            let controls_apply = controls_opts.clone();
            let cache_apply = cache_opts.clone();
            let suppress_apply = suppress_opts.clone();
            glib::spawn_future_local(async move {
                if let Ok(options) = rx.recv().await {
                    cache_apply
                        .borrow_mut()
                        .insert(section_key.clone(), options.clone());
                    apply_filter_options_to_controls(&controls_apply, &options, &suppress_apply);
                }
            });
        });
    }

    container
}

fn populate_filter_combo(combo: &ComboBoxText, all_label: &str, options: &[FilterOption]) {
    combo.remove_all();
    combo.append(Some(""), all_label);
    for option in options {
        combo.append(Some(&option.key), &option.title);
    }
    combo.set_active_id(Some(""));
}

fn apply_filter_options_to_controls(
    controls: &FilterControls,
    options: &SectionFilterOptions,
    suppress_events: &Cell<bool>,
) {
    suppress_events.set(true);
    populate_filter_combo(&controls.genre, "All Genres", &options.genres);
    populate_filter_combo(&controls.year, "All Years", &options.years);
    populate_filter_combo(
        &controls.content_rating,
        "All Ratings",
        &options.content_ratings,
    );
    populate_filter_combo(&controls.resolution, "All Resolutions", &options.resolutions);
    populate_filter_combo(
        &controls.audio_language,
        "All Audio Languages",
        &options.audio_languages,
    );
    suppress_events.set(false);
}

fn apply_sidebar_selection(state: &Arc<Mutex<AppState>>, section_list: &ListBox) {
    let selected_key = {
        let s = state.lock().unwrap();
        s.selected_library_key.clone()
    };

    if let Some(key) = selected_key {
        section_list.set_visible(false);
        let mut idx = 0;
        while let Some(row) = section_list.row_at_index(idx) {
            if row.widget_name() == key {
                section_list.select_row(Some(&row));
                break;
            }
            idx += 1;
        }
    } else {
        section_list.set_visible(true);
    }
}

fn load_section_items(
    state: &Arc<Mutex<AppState>>,
    grid_area: &GtkBox,
    section_key: &str,
    filter: &LibraryFilter,
    section_type: Option<&str>,
) {
    while let Some(child) = grid_area.first_child() {
        grid_area.remove(&child);
    }

    let spinner = Spinner::new();
    spinner.set_spinning(true);
    spinner.set_halign(gtk4::Align::Center);
    spinner.set_valign(gtk4::Align::Center);
    spinner.set_vexpand(true);
    grid_area.append(&spinner);

    let (token, base_url) = {
        let s = state.lock().unwrap();
        match s.token.clone().zip(s.base_url().map(String::from)) {
            Some(pair) => pair,
            None => return,
        }
    };

    let key = section_key.to_string();
    let section_type = section_type.unwrap_or("movie").to_string();
    let filter_owned = filter.clone();
    let (tx, rx) = async_channel::unbounded::<(
        Vec<simplex_core::api::library::MetadataItem>,
        String,
        String,
        String,
    )>();

    let bu = base_url.clone();
    let tk = token.clone();
    crate::app::runtime().spawn(async move {
        match simplex_core::api::library::get_section_items_filtered(&bu, &tk, &key, &filter_owned)
            .await
        {
            Ok(items) => {
                let _ = tx.send((items, bu, tk, section_type)).await;
            }
            Err(e) => tracing::warn!("Failed to load section items: {e}"),
        }
    });

    let grid = grid_area.clone();
    let spin = spinner.clone();
    let state_click = state.clone();
    glib::spawn_future_local(async move {
        if let Ok((items, base_url, token, section_type)) = rx.recv().await {
            spin.set_visible(false);
            match section_type.as_str() {
                "show" => render_show_layout(&grid, &items, &base_url, &token, state_click.clone()),
                "artist" => render_music_layout(&grid, &items, &base_url, &token, state_click.clone()),
                _ => render_movie_layout(&grid, &items, &base_url, &token, state_click.clone()),
            }
        }
    });
}

fn render_movie_layout(
    grid: &GtkBox,
    items: &[simplex_core::api::library::MetadataItem],
    base_url: &str,
    token: &str,
    state: Arc<Mutex<AppState>>,
) {
    let on_click: Rc<dyn Fn(&str)> = Rc::new(move |key: &str| {
        crate::window::navigate_to_detail(&state, key, "library");
    });
    let poster_grid = PosterGrid::new();
    poster_grid.add_metadata_items_interactive(items, base_url, token, on_click);
    grid.append(&poster_grid.widget);
}

fn render_show_layout(
    grid: &GtkBox,
    items: &[simplex_core::api::library::MetadataItem],
    base_url: &str,
    token: &str,
    state: Arc<Mutex<AppState>>,
) {
    let on_click: Rc<dyn Fn(&str)> = Rc::new(move |key: &str| {
        crate::window::navigate_to_detail(&state, key, "library");
    });
    let show_grid = PosterGrid::new();
    show_grid.add_metadata_items_interactive(items, base_url, token, on_click);
    grid.append(&show_grid.widget);
}

fn render_music_layout(
    grid: &GtkBox,
    items: &[simplex_core::api::library::MetadataItem],
    base_url: &str,
    token: &str,
    state: Arc<Mutex<AppState>>,
) {
    let section_label = Label::new(Some("Artists"));
    section_label.add_css_class("title-2");
    section_label.set_halign(gtk4::Align::Start);
    grid.append(&section_label);

    let on_click: Rc<dyn Fn(&str)> = Rc::new(move |key: &str| {
        crate::window::navigate_to_detail(&state, key, "library");
    });
    let music_grid = PosterGrid::new_square();
    music_grid.add_metadata_items_interactive(items, base_url, token, on_click);
    grid.append(&music_grid.widget);
}
