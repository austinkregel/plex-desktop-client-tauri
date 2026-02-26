mod app;
mod window;
mod views;
mod player;
mod widgets;

fn main() {
    std::panic::set_hook(Box::new(|info| {
        let payload = if let Some(s) = info.payload().downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "unknown panic".to_string()
        };

        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "unknown location".to_string());

        eprintln!("\n=== SIMPLEX PANIC ===");
        eprintln!("Location: {}", location);
        eprintln!("Message: {}", payload);
        eprintln!("Backtrace:\n{}", std::backtrace::Backtrace::force_capture());
        eprintln!("=== END PANIC ===\n");
    }));

    tracing_subscriber::fmt::init();

    gstreamer::init().expect("Failed to initialize GStreamer");
    gstgtk4::plugin_register_static()
        .expect("Failed to register gtk4paintablesink plugin");

    app::run();
}
