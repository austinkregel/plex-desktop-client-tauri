use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Label, Orientation, Overlay, Picture, ScrolledWindow};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::OnceLock;
use tokio::sync::Semaphore;

static DOWNLOAD_SEMAPHORE: OnceLock<Semaphore> = OnceLock::new();

fn download_semaphore() -> &'static Semaphore {
    DOWNLOAD_SEMAPHORE.get_or_init(|| Semaphore::new(6))
}

use simplex_core::ui_utils::{
    CARD_WIDTH, LANDSCAPE_CARD_HEIGHT, LANDSCAPE_CARD_WIDTH, POSTER_HEIGHT, SQUARE_CARD_SIZE,
    VIEWPORT_MARGIN,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaCardStyle {
    Poster,
    Square,
    Landscape,
}

impl MediaCardStyle {
    fn dimensions(self) -> (i32, i32) {
        match self {
            Self::Poster => (CARD_WIDTH, POSTER_HEIGHT),
            Self::Square => (SQUARE_CARD_SIZE, SQUARE_CARD_SIZE),
            Self::Landscape => (LANDSCAPE_CARD_WIDTH, LANDSCAPE_CARD_HEIGHT),
        }
    }
}

pub struct MediaCard {
    pub widget: GtkBox,
}

impl MediaCard {
    pub fn new(title: &str, subtitle: Option<&str>, thumb_url: Option<&str>) -> Self {
        Self::new_with_style(title, subtitle, thumb_url, MediaCardStyle::Poster)
    }

    pub fn new_with_style(
        title: &str,
        subtitle: Option<&str>,
        thumb_url: Option<&str>,
        style: MediaCardStyle,
    ) -> Self {
        let card_box = GtkBox::new(Orientation::Vertical, 4);
        let (card_w, card_h) = style.dimensions();
        card_box.set_width_request(card_w);
        card_box.add_css_class("card");
        card_box.set_margin_start(4);
        card_box.set_margin_end(4);
        card_box.set_margin_top(4);
        card_box.set_margin_bottom(4);

        let poster = Overlay::new();
        poster.set_overflow(gtk4::Overflow::Hidden);
        let sizer = GtkBox::new(Orientation::Vertical, 0);
        sizer.set_size_request(card_w, card_h);
        sizer.add_css_class("poster-placeholder");
        poster.set_child(Some(&sizer));

        let picture = Picture::new();
        picture.set_content_fit(gtk4::ContentFit::Cover);
        picture.set_can_shrink(true);
        poster.add_overlay(&picture);
        card_box.append(&poster);

        if let Some(url) = thumb_url {
            setup_lazy_loading(&card_box, &picture, url);
        }

        let title_label = Label::new(Some(title));
        title_label.set_halign(gtk4::Align::Start);
        title_label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        title_label.set_max_width_chars(20);
        title_label.set_margin_start(4);
        title_label.set_margin_end(4);
        title_label.add_css_class("heading");
        card_box.append(&title_label);

        if let Some(sub) = subtitle {
            let sub_label = Label::new(Some(sub));
            sub_label.set_halign(gtk4::Align::Start);
            sub_label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
            sub_label.set_max_width_chars(20);
            sub_label.set_margin_start(4);
            sub_label.set_margin_end(4);
            sub_label.add_css_class("dim-label");
            card_box.append(&sub_label);
        }

        Self { widget: card_box }
    }
}

fn setup_lazy_loading(card: &GtkBox, picture: &Picture, url: &str) {
    let url = Rc::new(url.to_string());
    let pic = picture.clone();
    let loaded = Rc::new(Cell::new(false));
    let scroll_connected = Rc::new(Cell::new(false));

    let url_c = url.clone();
    let pic_c = pic.clone();
    let loaded_c = loaded.clone();
    let connected_c = scroll_connected.clone();

    card.connect_map(move |widget| {
        if loaded_c.get() || connected_c.get() {
            return;
        }
        connected_c.set(true);

        let scroll = widget
            .ancestor(ScrolledWindow::static_type())
            .and_then(|w| w.downcast::<ScrolledWindow>().ok());

        match scroll {
            Some(scroll) => {
                connect_scroll_lazy_load(&scroll, &pic_c, &url_c, &loaded_c);
            }
            None => {
                loaded_c.set(true);
                load_thumb_async(&url_c, &pic_c);
            }
        }
    });
}

fn connect_scroll_lazy_load(
    scroll: &ScrolledWindow,
    pic: &Picture,
    url: &Rc<String>,
    loaded: &Rc<Cell<bool>>,
) {
    let try_load = {
        let scroll = scroll.clone();
        let pic = pic.clone();
        let url = url.clone();
        let loaded = loaded.clone();
        move || {
            if loaded.get() {
                return;
            }
            if is_in_viewport(&scroll, &pic) {
                loaded.set(true);
                load_thumb_async(&url, &pic);
            }
        }
    };

    try_load();

    if !loaded.get() {
        let try_load_on_scroll = try_load.clone();
        scroll.vadjustment().connect_value_changed(move |_| {
            try_load_on_scroll();
        });

        // Also check after layout settles (first frame)
        let try_load_idle = try_load;
        glib::idle_add_local_once(move || {
            try_load_idle();
        });
    }
}

fn is_in_viewport(scroll: &ScrolledWindow, widget: &Picture) -> bool {
    let adj = scroll.vadjustment();
    let content = match scroll.child() {
        Some(c) => c,
        None => return false,
    };

    match widget_y_in_ancestor(widget, &content) {
        Some(widget_y) => simplex_core::ui_utils::is_rect_in_viewport(
            adj.value(),
            adj.page_size(),
            widget_y,
            widget.height() as f64,
            VIEWPORT_MARGIN,
        ),
        None => false,
    }
}

/// Walks up the widget tree from `widget` to `ancestor`, accumulating
/// allocation y-offsets to compute the widget's vertical position
/// within the ancestor's coordinate space.
fn widget_y_in_ancestor(
    widget: &impl IsA<gtk4::Widget>,
    ancestor: &gtk4::Widget,
) -> Option<f64> {
    let mut y = 0.0;
    let mut current: gtk4::Widget = widget.clone().upcast();

    loop {
        if &current == ancestor {
            return Some(y);
        }
        y += current.allocation().y() as f64;
        match current.parent() {
            Some(p) => current = p,
            None => return None,
        }
    }
}

fn load_thumb_async(url: &str, picture: &Picture) {
    let url = url.to_string();
    let pic = picture.clone();
    let (tx, rx) = async_channel::unbounded::<Vec<u8>>();

    crate::app::runtime().spawn(async move {
        let _permit = match download_semaphore().acquire().await {
            Ok(p) => p,
            Err(_) => return,
        };

        let resp = match reqwest::get(&url).await {
            Ok(r) => r,
            Err(e) => {
                tracing::debug!("Thumbnail download failed: {}", e);
                return;
            }
        };

        if !resp.status().is_success() {
            tracing::debug!("Thumbnail HTTP {}: {}", resp.status(), url);
            return;
        }

        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if !simplex_core::ui_utils::is_image_content_type(content_type) {
            tracing::debug!("Thumbnail not an image ({}): {}", content_type, url);
            return;
        }

        match resp.bytes().await {
            Ok(bytes) if !bytes.is_empty() => {
                let _ = tx.send(bytes.to_vec()).await;
            }
            Ok(_) => tracing::debug!("Thumbnail empty response: {}", url),
            Err(e) => tracing::debug!("Thumbnail read error: {}", e),
        }
    });

    glib::spawn_future_local(async move {
        if let Ok(bytes) = rx.recv().await {
            if pic.parent().is_none() {
                return;
            }

            let g_bytes = glib::Bytes::from(&bytes);
            match gdk4::Texture::from_bytes(&g_bytes) {
                Ok(texture) => {
                    pic.set_paintable(Some(&texture));
                }
                Err(e) => {
                    tracing::debug!("Texture decode error: {}", e);
                }
            }
        }
    });
}
