//! Pure utility functions for UI logic, testable without a display server.

/// Checks whether a rectangle at `widget_y` with `widget_height` is visible
/// (or within the preload margin) of a scroll viewport.
///
/// The viewport starts at `scroll_y` and extends `page_size` pixels downward.
/// Items within `margin` pixels beyond the viewport edges are considered visible
/// to support preloading content before it scrolls into view.
pub fn is_rect_in_viewport(
    scroll_y: f64,
    page_size: f64,
    widget_y: f64,
    widget_height: f64,
    margin: f64,
) -> bool {
    if page_size <= 0.0 || widget_height <= 0.0 {
        return false;
    }
    let viewport_top = scroll_y - margin;
    let viewport_bottom = scroll_y + page_size + margin;
    let widget_bottom = widget_y + widget_height;

    widget_bottom > viewport_top && widget_y < viewport_bottom
}

/// Validates that an HTTP Content-Type header value represents an image.
pub fn is_image_content_type(content_type: &str) -> bool {
    content_type.starts_with("image/")
}

/// Format a duration in seconds as `H:MM:SS` or `M:SS`.
pub fn format_time(seconds: f64) -> String {
    let total = seconds as u64;
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    if h > 0 {
        format!("{}:{:02}:{:02}", h, m, s)
    } else {
        format!("{}:{:02}", m, s)
    }
}

/// Standard poster card dimensions (2:3 aspect ratio).
pub const CARD_WIDTH: i32 = 180;
pub const POSTER_HEIGHT: i32 = 270;
pub const SQUARE_CARD_SIZE: i32 = 180;
pub const LANDSCAPE_CARD_WIDTH: i32 = 240;
pub const LANDSCAPE_CARD_HEIGHT: i32 = 135;

/// Pixels beyond the viewport edge to preload images.
pub const VIEWPORT_MARGIN: f64 = 400.0;

#[cfg(test)]
mod tests {
    use super::*;

    // Viewport: scroll_y=0, page_size=800 (visible from y=0..800)
    // Margin: 400 (preload zone from y=-400..1200)

    // -- Basic visibility --

    #[test]
    fn test_widget_fully_visible() {
        assert!(is_rect_in_viewport(0.0, 800.0, 100.0, 270.0, 400.0));
    }

    #[test]
    fn test_widget_at_top_of_viewport() {
        assert!(is_rect_in_viewport(0.0, 800.0, 0.0, 270.0, 400.0));
    }

    #[test]
    fn test_widget_at_bottom_of_viewport() {
        assert!(is_rect_in_viewport(0.0, 800.0, 530.0, 270.0, 400.0));
    }

    // -- Scrolled viewport --

    #[test]
    fn test_widget_visible_after_scroll() {
        // Scrolled 1000px down, viewport shows y=1000..1800
        assert!(is_rect_in_viewport(1000.0, 800.0, 1200.0, 270.0, 400.0));
    }

    #[test]
    fn test_widget_above_scrolled_viewport() {
        // Scrolled 2000px down; widget at y=100 is way above viewport (2000-400=1600)
        assert!(!is_rect_in_viewport(2000.0, 800.0, 100.0, 270.0, 400.0));
    }

    #[test]
    fn test_widget_below_scrolled_viewport() {
        // Scrolled 0px; widget at y=5000 is way below viewport (0+800+400=1200)
        assert!(!is_rect_in_viewport(0.0, 800.0, 5000.0, 270.0, 400.0));
    }

    // -- Margin / preload zone --

    #[test]
    fn test_widget_in_preload_zone_below_viewport() {
        // Viewport 0..800, margin 400 → preload to 1200
        // Widget at y=900 (below visible area but within margin)
        assert!(is_rect_in_viewport(0.0, 800.0, 900.0, 270.0, 400.0));
    }

    #[test]
    fn test_widget_in_preload_zone_above_viewport() {
        // Scrolled 1000px, viewport 1000..1800, margin extends to 600..2200
        // Widget at y=700 (above visible but within margin)
        assert!(is_rect_in_viewport(1000.0, 800.0, 700.0, 270.0, 400.0));
    }

    #[test]
    fn test_widget_just_outside_preload_zone_below() {
        // Viewport 0..800, margin 400 → preload to 1200
        // Widget starts at y=1200 (bottom edge exactly at preload boundary)
        assert!(!is_rect_in_viewport(0.0, 800.0, 1200.0, 270.0, 400.0));
    }

    #[test]
    fn test_widget_just_inside_preload_zone_below() {
        // Widget starts at y=1199 (just barely inside preload zone)
        assert!(is_rect_in_viewport(0.0, 800.0, 1199.0, 270.0, 400.0));
    }

    #[test]
    fn test_widget_just_outside_preload_zone_above() {
        // Scrolled 1000px, preload starts at 600
        // Widget ends at y=600 (widget_y=330, height=270 → bottom=600)
        assert!(!is_rect_in_viewport(1000.0, 800.0, 330.0, 270.0, 400.0));
    }

    #[test]
    fn test_widget_just_inside_preload_zone_above() {
        // Widget bottom at y=601 (just barely overlapping preload zone)
        assert!(is_rect_in_viewport(1000.0, 800.0, 331.0, 270.0, 400.0));
    }

    // -- Partial visibility --

    #[test]
    fn test_widget_partially_visible_top_edge() {
        // Widget straddles the top of the viewport
        // Scrolled 200, viewport 200..1000; widget at y=50, h=270 → bottom=320 (visible)
        assert!(is_rect_in_viewport(200.0, 800.0, 50.0, 270.0, 0.0));
    }

    #[test]
    fn test_widget_partially_visible_bottom_edge() {
        // Widget straddles the bottom of the viewport
        // Viewport 0..800; widget at y=700, h=270 → bottom=970 (partially visible)
        assert!(is_rect_in_viewport(0.0, 800.0, 700.0, 270.0, 0.0));
    }

    // -- Zero margin --

    #[test]
    fn test_zero_margin_visible() {
        assert!(is_rect_in_viewport(0.0, 800.0, 100.0, 270.0, 0.0));
    }

    #[test]
    fn test_zero_margin_just_below_viewport() {
        // Without margin, widget starting at y=800 is exactly outside
        assert!(!is_rect_in_viewport(0.0, 800.0, 800.0, 270.0, 0.0));
    }

    #[test]
    fn test_zero_margin_just_above_viewport() {
        // Widget ends exactly at viewport top
        // Widget at y=-270, h=270 → bottom=0, viewport starts at 0
        assert!(!is_rect_in_viewport(0.0, 800.0, -270.0, 270.0, 0.0));
    }

    // -- Edge cases --

    #[test]
    fn test_zero_page_size_always_false() {
        assert!(!is_rect_in_viewport(0.0, 0.0, 100.0, 270.0, 400.0));
    }

    #[test]
    fn test_negative_page_size_always_false() {
        assert!(!is_rect_in_viewport(0.0, -100.0, 100.0, 270.0, 400.0));
    }

    #[test]
    fn test_zero_widget_height_always_false() {
        // Zero-height means the widget hasn't been laid out yet by GTK;
        // we must not consider it visible or we'll eagerly load all images.
        assert!(!is_rect_in_viewport(0.0, 800.0, 0.0, 0.0, 400.0));
    }

    #[test]
    fn test_zero_widget_height_outside_viewport() {
        assert!(!is_rect_in_viewport(0.0, 800.0, 5000.0, 0.0, 400.0));
    }

    #[test]
    fn test_negative_widget_height_always_false() {
        assert!(!is_rect_in_viewport(0.0, 800.0, 100.0, -10.0, 400.0));
    }

    #[test]
    fn test_large_scroll_offset() {
        // Scrolled very far; widget at matching position is visible
        assert!(is_rect_in_viewport(50000.0, 800.0, 50100.0, 270.0, 400.0));
    }

    #[test]
    fn test_large_scroll_offset_widget_far_above() {
        assert!(!is_rect_in_viewport(50000.0, 800.0, 100.0, 270.0, 400.0));
    }

    // -- Content-type validation --

    #[test]
    fn test_image_jpeg() {
        assert!(is_image_content_type("image/jpeg"));
    }

    #[test]
    fn test_image_png() {
        assert!(is_image_content_type("image/png"));
    }

    #[test]
    fn test_image_webp() {
        assert!(is_image_content_type("image/webp"));
    }

    #[test]
    fn test_image_with_charset() {
        assert!(is_image_content_type("image/png; charset=utf-8"));
    }

    #[test]
    fn test_text_html_not_image() {
        assert!(!is_image_content_type("text/html"));
    }

    #[test]
    fn test_application_json_not_image() {
        assert!(!is_image_content_type("application/json"));
    }

    #[test]
    fn test_empty_content_type_not_image() {
        assert!(!is_image_content_type(""));
    }

    #[test]
    fn test_text_xml_not_image() {
        assert!(!is_image_content_type("text/xml"));
    }

    // -- format_time --

    #[test]
    fn test_format_time_zero() {
        assert_eq!(format_time(0.0), "0:00");
    }

    #[test]
    fn test_format_time_seconds_only() {
        assert_eq!(format_time(45.0), "0:45");
    }

    #[test]
    fn test_format_time_one_minute() {
        assert_eq!(format_time(60.0), "1:00");
    }

    #[test]
    fn test_format_time_minutes_and_seconds() {
        assert_eq!(format_time(125.0), "2:05");
    }

    #[test]
    fn test_format_time_under_one_hour() {
        assert_eq!(format_time(3599.0), "59:59");
    }

    #[test]
    fn test_format_time_exactly_one_hour() {
        assert_eq!(format_time(3600.0), "1:00:00");
    }

    #[test]
    fn test_format_time_hours_minutes_seconds() {
        assert_eq!(format_time(3723.0), "1:02:03");
    }

    #[test]
    fn test_format_time_multi_hour() {
        assert_eq!(format_time(7384.0), "2:03:04");
    }

    #[test]
    fn test_format_time_fractional_seconds_truncated() {
        assert_eq!(format_time(90.7), "1:30");
    }

    #[test]
    fn test_format_time_large_duration() {
        // 10 hours
        assert_eq!(format_time(36000.0), "10:00:00");
    }

    #[test]
    fn test_format_time_pads_seconds() {
        assert_eq!(format_time(61.0), "1:01");
    }

    #[test]
    fn test_format_time_pads_minutes_in_hour_format() {
        assert_eq!(format_time(3660.0), "1:01:00");
    }

    // -- Poster dimensions sanity --

    #[test]
    fn test_poster_aspect_ratio_is_2_3() {
        let ratio = POSTER_HEIGHT as f64 / CARD_WIDTH as f64;
        assert!(
            (ratio - 1.5).abs() < 0.01,
            "Expected 2:3 ratio (1.5), got {}",
            ratio
        );
    }

    #[test]
    fn test_viewport_margin_positive() {
        assert!(VIEWPORT_MARGIN > 0.0);
    }
}
