use libadwaita::prelude::*;
use libadwaita::Application;
use std::sync::OnceLock;

const APP_ID: &str = "com.austinkregel.simplex";

static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();

pub fn runtime() -> &'static tokio::runtime::Runtime {
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("Failed to create tokio runtime")
    })
}

fn load_css() {
    let provider = gtk4::CssProvider::new();
    provider.load_from_string(
        ".player-view { background-color: black; }",
    );
    gtk4::style_context_add_provider_for_display(
        &gdk4::Display::default().expect("Could not get default display"),
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

pub fn run() {
    let _ = runtime();

    let app = Application::builder()
        .application_id(APP_ID)
        .build();

    app.connect_activate(|app| {
        load_css();
        crate::window::build_window(app);
    });

    app.run();
}
