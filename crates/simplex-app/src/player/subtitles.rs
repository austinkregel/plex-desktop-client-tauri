//! Subtitle track management with language detection from GStreamer tags.

use gstreamer as gst;
use gstreamer::prelude::*;

use super::pipeline::PlayerPipeline;

pub struct SubtitleManager;

impl SubtitleManager {
    /// List available subtitle tracks, extracting language from GStreamer tags.
    pub fn list_tracks(pipeline: &PlayerPipeline) -> Vec<SubtitleInfo> {
        let count = pipeline.subtitle_track_count();
        let element = pipeline.element();
        let mut tracks = Vec::new();
        for i in 0..count {
            let (language, title) = get_subtitle_tags(element, i);
            tracks.push(SubtitleInfo {
                index: i,
                language,
                title,
            });
        }
        tracks
    }

    /// Select a subtitle track.
    pub fn select_track(pipeline: &PlayerPipeline, index: i32) {
        pipeline.set_subtitle_track(index);
    }

    /// Disable subtitles.
    pub fn disable(pipeline: &PlayerPipeline) {
        pipeline.set_subtitle_track(-1);
    }
}

fn get_subtitle_tags(element: &gst::Element, index: i32) -> (Option<String>, Option<String>) {
    let tags: Option<gst::TagList> = element
        .emit_by_name("get-text-tags", &[&index]);
    match tags {
        Some(t) => {
            let lang = t.get::<gst::tags::LanguageCode>()
                .map(|v| v.get().to_string());
            let title = t.get::<gst::tags::Title>()
                .map(|v| v.get().to_string());
            (lang, title)
        }
        None => (None, None),
    }
}

pub struct SubtitleInfo {
    pub index: i32,
    pub language: Option<String>,
    pub title: Option<String>,
}
