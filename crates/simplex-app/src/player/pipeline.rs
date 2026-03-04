//! GStreamer playbin3 + gtk4paintablesink pipeline.
//!
//! Uses GStreamer's playbin3 element with gtk4paintablesink for software-rendered
//! video in a GTK4 Picture widget. Hardware decoders are explicitly downranked
//! to avoid VA memory / DMA-BUF format issues with the software sink path.

use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer::{Element, MessageView, State};
use gtk4::prelude::*;
use gtk4::Picture;
use std::sync::{Arc, Mutex};

/// Trait abstracting the pipeline API for testability.
/// Consumers like `SubtitleManager` and `TrackMonitor` operate through this
/// interface so they can be tested with a mock implementation.
pub trait PipelineApi {
    fn current_audio_track(&self) -> i32;
    fn current_subtitle_track(&self) -> i32;
    fn audio_track_count(&self) -> i32;
    fn subtitle_track_count(&self) -> i32;
    fn audio_language(&self, index: i32) -> Option<String>;
    fn audio_title(&self, index: i32) -> Option<String>;
    fn audio_codec(&self, index: i32) -> Option<String>;
    fn subtitle_language(&self, index: i32) -> Option<String>;
    fn subtitle_title(&self, index: i32) -> Option<String>;
    fn set_audio_track(&self, index: i32);
    fn set_subtitle_track(&self, index: i32);
    fn pause(&self);
    fn play(&self);
    fn stop(&self);
    fn position(&self) -> Option<f64>;
    fn duration(&self) -> Option<f64>;
    fn is_playing(&self) -> bool;
    fn set_volume(&self, volume: f64);
    fn volume(&self) -> f64;
}

#[derive(Default)]
pub(crate) struct StreamCache {
    pub audio: Vec<gst::Stream>,
    pub video: Vec<gst::Stream>,
    pub text: Vec<gst::Stream>,
}

pub struct PlayerPipeline {
    pipeline: Element,
    paintable_sink: Element,
    pub picture: Picture,
    uri: Option<String>,
    _bus_watch_guard: Option<gst::bus::BusWatchGuard>,
    streams: Arc<Mutex<StreamCache>>,
    uses_playbin3_signals: bool,
}

impl PlayerPipeline {
    pub fn new() -> Result<Self, String> {
        // Downrank hardware video decoders (VA-API, VAAPI) so playbin3 uses
        // software decoders. HW decoders output VA/DMA-BUF memory that the
        // software sink path cannot read, producing green/corrupt frames.
        for factory in gst::ElementFactory::factories_with_type(
            gst::ElementFactoryType::DECODER | gst::ElementFactoryType::MEDIA_VIDEO,
            gst::Rank::MARGINAL,
        ) {
            let name = factory.name();
            if name.starts_with("va") {
                factory.set_rank(gst::Rank::NONE);
                tracing::debug!("Downranked HW decoder: {}", name);
            }
        }

        let paintable_sink = gst::ElementFactory::make("gtk4paintablesink")
            .build()
            .map_err(|e| format!(
                "gtk4paintablesink not found. Install gstreamer1.0-gtk4 \
                 (or gstreamer1.0-plugins-rs): {e}"
            ))?;

        let paintable: gdk4::Paintable = paintable_sink.property("paintable");
        let picture = Picture::new();
        picture.set_paintable(Some(&paintable));
        picture.set_content_fit(gtk4::ContentFit::Contain);
        picture.set_can_shrink(true);
        picture.set_vexpand(true);
        picture.set_hexpand(true);

        // Build a video sink bin: videoconvert ensures frames are converted
        // from whatever the decoder outputs (I420, NV12, etc.) into a format
        // gtk4paintablesink can display correctly (BGRA/RGBA).
        let convert = gst::ElementFactory::make("videoconvert")
            .build()
            .map_err(|e| format!("videoconvert not found: {e}"))?;

        let bin = gst::Bin::builder().name("video-sink-bin").build();
        bin.add_many([&convert, &paintable_sink])
            .map_err(|e| format!("Failed to add elements to video sink bin: {e}"))?;
        convert.link(&paintable_sink)
            .map_err(|e| format!("Failed to link videoconvert to paintable sink: {e}"))?;

        let sink_pad = convert.static_pad("sink")
            .ok_or("videoconvert has no sink pad")?;
        let ghost_pad = gst::GhostPad::with_target(&sink_pad)
            .map_err(|e| format!("Failed to create ghost pad: {e}"))?;
        bin.add_pad(&ghost_pad)
            .map_err(|e| format!("Failed to add ghost pad to bin: {e}"))?;

        let pipeline = gst::ElementFactory::make("playbin3")
            .property("video-sink", &bin)
            .build()
            .map_err(|e| format!("playbin3 not found. Install gstreamer1.0-plugins-base: {e}"))?;

        pipeline.set_property("buffer-size", 10 * 1024 * 1024i32);

        let uses_playbin3_signals = glib::subclass::SignalId::lookup(
            "get-audio-tags",
            pipeline.type_(),
        )
        .is_some();

        Ok(Self {
            pipeline,
            paintable_sink,
            picture,
            uri: None,
            _bus_watch_guard: None,
            streams: Arc::new(Mutex::new(StreamCache::default())),
            uses_playbin3_signals,
        })
    }

    pub fn set_uri(&mut self, uri: &str) {
        self.uri = Some(uri.to_string());
        self.streams.lock().unwrap().clear();
        self.pipeline.set_property("uri", uri);
    }

    pub fn play(&self) {
        let _ = self.pipeline.set_state(State::Playing);
        tracing::info!("Playback started");
    }

    pub fn pause(&self) {
        let _ = self.pipeline.set_state(State::Paused);
        tracing::info!("Playback paused");
    }

    pub fn stop(&self) {
        let _ = self.pipeline.set_state(State::Null);
        tracing::info!("Playback stopped");
    }

    pub fn toggle_play_pause(&self) {
        let (_, current, _) = self.pipeline.state(gst::ClockTime::ZERO);
        match current {
            State::Playing => self.pause(),
            State::Paused => self.play(),
            _ => self.play(),
        }
    }

    pub fn seek(&self, position_secs: f64) {
        let pos = gst::ClockTime::from_nseconds((position_secs * 1_000_000_000.0) as u64);
        let _ = self.pipeline.seek_simple(
            gst::SeekFlags::FLUSH | gst::SeekFlags::KEY_UNIT,
            pos,
        );
    }

    pub fn position(&self) -> Option<f64> {
        self.pipeline
            .query_position::<gst::ClockTime>()
            .map(|p| p.nseconds() as f64 / 1_000_000_000.0)
    }

    pub fn duration(&self) -> Option<f64> {
        self.pipeline
            .query_duration::<gst::ClockTime>()
            .map(|d| d.nseconds() as f64 / 1_000_000_000.0)
    }

    pub fn set_volume(&self, volume: f64) {
        self.pipeline.set_property("volume", volume.clamp(0.0, 1.5));
    }

    pub fn volume(&self) -> f64 {
        self.pipeline.property::<f64>("volume")
    }

    pub fn is_playing(&self) -> bool {
        let (_, current, _) = self.pipeline.state(gst::ClockTime::ZERO);
        current == State::Playing
    }

    // ---- Track counts -------------------------------------------------------

    pub fn audio_track_count(&self) -> i32 {
        let count = self.try_i32_property("n-audio").unwrap_or(0);
        if count > 0 {
            return count;
        }
        let cached = self.streams.lock().unwrap().audio.len() as i32;
        if cached > 0 {
            return cached;
        }
        if self.is_playing() {
            tracing::debug!("n-audio is 0 while playing; assuming 1 audio track");
            1
        } else {
            0
        }
    }

    pub fn subtitle_track_count(&self) -> i32 {
        let count = self.try_i32_property("n-text").unwrap_or(0);
        if count > 0 {
            return count;
        }
        let cached = self.streams.lock().unwrap().text.len() as i32;
        if cached > 0 {
            return cached;
        }
        0
    }

    // ---- Current track index ------------------------------------------------

    pub fn current_audio_track(&self) -> i32 {
        self.try_i32_property("current-audio").unwrap_or(-1)
    }

    pub fn current_subtitle_track(&self) -> i32 {
        self.try_i32_property("current-text").unwrap_or(-1)
    }

    // ---- Track selection ----------------------------------------------------

    pub fn set_audio_track(&self, index: i32) {
        tracing::info!("Selecting audio track {}", index);
        if self.pipeline.find_property("current-audio").is_some() {
            self.pipeline.set_property("current-audio", index);
        }
        self.send_select_streams_for_audio(index as usize);
    }

    pub fn set_subtitle_track(&self, index: i32) {
        if index < 0 {
            tracing::info!("Disabling subtitles");
        } else {
            tracing::info!("Selecting subtitle track {}", index);
        }
        if self.pipeline.find_property("current-text").is_some() {
            self.pipeline.set_property("current-text", index);
        }
        let text_idx = if index >= 0 { Some(index as usize) } else { None };
        self.send_select_streams_for_text(text_idx);
    }

    /// Send a `SelectStreams` event to playbin3 with the desired audio stream
    /// while keeping the current video and text streams.
    fn send_select_streams_for_audio(&self, audio_idx: usize) {
        let cache = self.streams.lock().unwrap();
        if cache.audio.is_empty() && cache.video.is_empty() {
            return;
        }
        let mut ids: Vec<String> = Vec::new();
        if let Some(v) = cache.video.first() {
            if let Some(id) = v.stream_id() {
                ids.push(id.to_string());
            }
        }
        if let Some(a) = cache.audio.get(audio_idx) {
            if let Some(id) = a.stream_id() {
                ids.push(id.to_string());
            }
        }
        let current_text = self.try_i32_property("current-text").unwrap_or(-1);
        if current_text >= 0 {
            if let Some(t) = cache.text.get(current_text as usize) {
                if let Some(id) = t.stream_id() {
                    ids.push(id.to_string());
                }
            }
        }
        drop(cache);
        self.send_select_streams(&ids);
    }

    /// Send a `SelectStreams` event to playbin3 with the desired text stream
    /// while keeping the current video and audio streams.
    fn send_select_streams_for_text(&self, text_idx: Option<usize>) {
        let cache = self.streams.lock().unwrap();
        if cache.audio.is_empty() && cache.video.is_empty() {
            return;
        }
        let mut ids: Vec<String> = Vec::new();
        if let Some(v) = cache.video.first() {
            if let Some(id) = v.stream_id() {
                ids.push(id.to_string());
            }
        }
        let current_audio = self.try_i32_property("current-audio").unwrap_or(0);
        if let Some(a) = cache.audio.get(current_audio.max(0) as usize) {
            if let Some(id) = a.stream_id() {
                ids.push(id.to_string());
            }
        }
        if let Some(idx) = text_idx {
            if let Some(t) = cache.text.get(idx) {
                if let Some(id) = t.stream_id() {
                    ids.push(id.to_string());
                }
            }
        }
        drop(cache);
        self.send_select_streams(&ids);
    }

    fn send_select_streams(&self, stream_ids: &[String]) {
        if stream_ids.is_empty() {
            return;
        }
        let id_refs: Vec<&str> = stream_ids.iter().map(|s| s.as_str()).collect();
        tracing::debug!("Sending SelectStreams: {:?}", id_refs);
        let event = gst::event::SelectStreams::new(&id_refs);
        if !self.pipeline.send_event(event) {
            tracing::debug!("SelectStreams event was not handled");
        }
    }

    // ---- Playback speed -----------------------------------------------------

    pub fn set_playback_speed(&self, rate: f64) {
        let rate = rate.clamp(0.25, 4.0);
        let position = self
            .pipeline
            .query_position::<gst::ClockTime>()
            .unwrap_or(gst::ClockTime::ZERO);

        let _ = self.pipeline.seek(
            rate,
            gst::SeekFlags::FLUSH | gst::SeekFlags::ACCURATE,
            gst::SeekType::Set,
            position,
            gst::SeekType::End,
            gst::ClockTime::ZERO,
        );
    }

    // ---- Tag queries (safe for playbin3) ------------------------------------

    pub fn element(&self) -> &Element {
        &self.pipeline
    }

    fn tags_by_signal(&self, signal: &str, index: i32) -> Option<gst::TagList> {
        if self.uses_playbin3_signals {
            return self.pipeline.emit_by_name(signal, &[&index]);
        }
        None
    }

    fn audio_tags(&self, index: i32) -> Option<gst::TagList> {
        if let Some(tags) = self.tags_by_signal("get-audio-tags", index) {
            return Some(tags);
        }
        let cache = self.streams.lock().unwrap();
        cache.audio.get(index as usize).and_then(|s| s.tags())
    }

    fn text_tags(&self, index: i32) -> Option<gst::TagList> {
        if let Some(tags) = self.tags_by_signal("get-text-tags", index) {
            return Some(tags);
        }
        let cache = self.streams.lock().unwrap();
        cache.text.get(index as usize).and_then(|s| s.tags())
    }

    pub fn audio_language(&self, index: i32) -> Option<String> {
        self.audio_tags(index)
            .and_then(|t| t.get::<gst::tags::LanguageCode>().map(|v| v.get().to_string()))
    }

    pub fn audio_title(&self, index: i32) -> Option<String> {
        self.audio_tags(index)
            .and_then(|t| t.get::<gst::tags::Title>().map(|v| v.get().to_string()))
    }

    pub fn audio_codec(&self, index: i32) -> Option<String> {
        self.audio_tags(index)
            .and_then(|t| t.get::<gst::tags::AudioCodec>().map(|v| v.get().to_string()))
    }

    pub fn subtitle_language(&self, index: i32) -> Option<String> {
        self.text_tags(index)
            .and_then(|t| t.get::<gst::tags::LanguageCode>().map(|v| v.get().to_string()))
    }

    pub fn subtitle_title(&self, index: i32) -> Option<String> {
        self.text_tags(index)
            .and_then(|t| t.get::<gst::tags::Title>().map(|v| v.get().to_string()))
    }

    // ---- Internal -----------------------------------------------------------

    pub fn paintable_sink(&self) -> &Element {
        &self.paintable_sink
    }

    fn try_i32_property(&self, name: &str) -> Option<i32> {
        self.pipeline.find_property(name)?;
        Some(self.pipeline.property::<i32>(name))
    }

    /// Connect to the bus for error/EOS/state-change handling.
    /// Intercepts `StreamCollection` messages to cache stream metadata
    /// (required for playbin3 which lacks `get-audio-tags` / `get-text-tags`
    /// action signals and needs `SelectStreams` events for track switching).
    pub fn connect_bus<F: Fn(MessageView) + 'static>(&mut self, callback: F) {
        let bus = self.pipeline.bus().expect("Pipeline has no bus");
        let streams_cache = self.streams.clone();
        let guard = bus.add_watch_local(move |_, msg| {
            if let MessageView::StreamCollection(sc) = msg.view() {
                let collection = sc.stream_collection();
                let mut cache = streams_cache.lock().unwrap();
                cache.clear();
                let n = collection.len() as u32;
                for i in 0..n {
                    if let Some(stream) = collection.stream(i) {
                        let st = stream.stream_type();
                        if st.contains(gst::StreamType::AUDIO) {
                            cache.audio.push(stream);
                        } else if st.contains(gst::StreamType::VIDEO) {
                            cache.video.push(stream);
                        } else if st.contains(gst::StreamType::TEXT) {
                            cache.text.push(stream);
                        }
                    }
                }
                tracing::debug!(
                    "StreamCollection: {} audio, {} video, {} text stream(s)",
                    cache.audio.len(),
                    cache.video.len(),
                    cache.text.len(),
                );
            }
            callback(msg.view());
            glib::ControlFlow::Continue
        })
        .expect("Failed to add bus watch");
        self._bus_watch_guard = Some(guard);
    }
}

impl StreamCache {
    fn clear(&mut self) {
        self.audio.clear();
        self.video.clear();
        self.text.clear();
    }
}

impl PipelineApi for PlayerPipeline {
    fn current_audio_track(&self) -> i32 { self.current_audio_track() }
    fn current_subtitle_track(&self) -> i32 { self.current_subtitle_track() }
    fn audio_track_count(&self) -> i32 { self.audio_track_count() }
    fn subtitle_track_count(&self) -> i32 { self.subtitle_track_count() }
    fn audio_language(&self, index: i32) -> Option<String> { self.audio_language(index) }
    fn audio_title(&self, index: i32) -> Option<String> { self.audio_title(index) }
    fn audio_codec(&self, index: i32) -> Option<String> { self.audio_codec(index) }
    fn subtitle_language(&self, index: i32) -> Option<String> { self.subtitle_language(index) }
    fn subtitle_title(&self, index: i32) -> Option<String> { self.subtitle_title(index) }
    fn set_audio_track(&self, index: i32) { self.set_audio_track(index) }
    fn set_subtitle_track(&self, index: i32) { self.set_subtitle_track(index) }
    fn pause(&self) { self.pause() }
    fn play(&self) { self.play() }
    fn stop(&self) { self.stop() }
    fn position(&self) -> Option<f64> { self.position() }
    fn duration(&self) -> Option<f64> { self.duration() }
    fn is_playing(&self) -> bool { self.is_playing() }
    fn set_volume(&self, volume: f64) { self.set_volume(volume) }
    fn volume(&self) -> f64 { self.volume() }
}

impl Drop for PlayerPipeline {
    fn drop(&mut self) {
        let _ = self.pipeline.set_state(State::Null);
    }
}

#[cfg(test)]
pub(crate) mod mock {
    use super::PipelineApi;
    use std::cell::Cell;

    pub struct MockPipeline {
        pub audio_count: i32,
        pub subtitle_count: i32,
        pub current_audio: i32,
        pub current_subtitle: i32,
        pub audio_languages: Vec<Option<String>>,
        pub audio_titles: Vec<Option<String>>,
        pub audio_codecs: Vec<Option<String>>,
        pub subtitle_languages: Vec<Option<String>>,
        pub subtitle_titles: Vec<Option<String>>,
        pub paused: Cell<bool>,
        pub playing: Cell<bool>,
        pub stopped: Cell<bool>,
        pub selected_audio: Cell<Option<i32>>,
        pub selected_subtitle: Cell<Option<i32>>,
        pub volume_val: Cell<f64>,
        pub position_val: Option<f64>,
        pub duration_val: Option<f64>,
    }

    impl MockPipeline {
        pub fn new() -> Self {
            Self {
                audio_count: 0,
                subtitle_count: 0,
                current_audio: -1,
                current_subtitle: -1,
                audio_languages: vec![],
                audio_titles: vec![],
                audio_codecs: vec![],
                subtitle_languages: vec![],
                subtitle_titles: vec![],
                paused: Cell::new(false),
                playing: Cell::new(false),
                stopped: Cell::new(false),
                selected_audio: Cell::new(None),
                selected_subtitle: Cell::new(None),
                volume_val: Cell::new(1.0),
                position_val: None,
                duration_val: None,
            }
        }
    }

    impl PipelineApi for MockPipeline {
        fn current_audio_track(&self) -> i32 { self.current_audio }
        fn current_subtitle_track(&self) -> i32 { self.current_subtitle }
        fn audio_track_count(&self) -> i32 { self.audio_count }
        fn subtitle_track_count(&self) -> i32 { self.subtitle_count }
        fn audio_language(&self, index: i32) -> Option<String> {
            self.audio_languages.get(index as usize).cloned().flatten()
        }
        fn audio_title(&self, index: i32) -> Option<String> {
            self.audio_titles.get(index as usize).cloned().flatten()
        }
        fn audio_codec(&self, index: i32) -> Option<String> {
            self.audio_codecs.get(index as usize).cloned().flatten()
        }
        fn subtitle_language(&self, index: i32) -> Option<String> {
            self.subtitle_languages.get(index as usize).cloned().flatten()
        }
        fn subtitle_title(&self, index: i32) -> Option<String> {
            self.subtitle_titles.get(index as usize).cloned().flatten()
        }
        fn set_audio_track(&self, index: i32) {
            self.selected_audio.set(Some(index));
        }
        fn set_subtitle_track(&self, index: i32) {
            self.selected_subtitle.set(Some(index));
        }
        fn pause(&self) { self.paused.set(true); self.playing.set(false); }
        fn play(&self) { self.playing.set(true); self.paused.set(false); }
        fn stop(&self) { self.stopped.set(true); self.playing.set(false); }
        fn position(&self) -> Option<f64> { self.position_val }
        fn duration(&self) -> Option<f64> { self.duration_val }
        fn is_playing(&self) -> bool { self.playing.get() }
        fn set_volume(&self, volume: f64) { self.volume_val.set(volume); }
        fn volume(&self) -> f64 { self.volume_val.get() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_cache_default_is_empty() {
        let cache = StreamCache::default();
        assert!(cache.audio.is_empty());
        assert!(cache.video.is_empty());
        assert!(cache.text.is_empty());
    }

    #[test]
    fn test_stream_cache_clear() {
        let mut cache = StreamCache::default();
        cache.clear();
        assert!(cache.audio.is_empty());
        assert!(cache.video.is_empty());
        assert!(cache.text.is_empty());
    }

    #[test]
    fn test_mock_pipeline_defaults() {
        let mock = mock::MockPipeline::new();
        assert_eq!(mock.audio_track_count(), 0);
        assert_eq!(mock.subtitle_track_count(), 0);
        assert_eq!(mock.current_audio_track(), -1);
        assert_eq!(mock.current_subtitle_track(), -1);
        assert!(!mock.is_playing());
        assert_eq!(mock.volume(), 1.0);
        assert!(mock.position().is_none());
        assert!(mock.duration().is_none());
    }

    #[test]
    fn test_mock_pipeline_play_pause_stop() {
        let mock = mock::MockPipeline::new();
        mock.play();
        assert!(mock.is_playing());
        mock.pause();
        assert!(!mock.is_playing());
        assert!(mock.paused.get());
        mock.stop();
        assert!(mock.stopped.get());
    }

    #[test]
    fn test_mock_pipeline_track_selection() {
        let mock = mock::MockPipeline::new();
        mock.set_audio_track(2);
        assert_eq!(mock.selected_audio.get(), Some(2));
        mock.set_subtitle_track(1);
        assert_eq!(mock.selected_subtitle.get(), Some(1));
    }

    #[test]
    fn test_mock_pipeline_volume() {
        let mock = mock::MockPipeline::new();
        mock.set_volume(0.5);
        assert_eq!(mock.volume(), 0.5);
    }

    #[test]
    fn test_mock_pipeline_audio_languages() {
        let mut mock = mock::MockPipeline::new();
        mock.audio_count = 2;
        mock.audio_languages = vec![Some("eng".to_string()), Some("spa".to_string())];
        assert_eq!(mock.audio_language(0), Some("eng".to_string()));
        assert_eq!(mock.audio_language(1), Some("spa".to_string()));
        assert_eq!(mock.audio_language(2), None);
    }

    #[test]
    fn test_mock_pipeline_subtitle_info() {
        let mut mock = mock::MockPipeline::new();
        mock.subtitle_count = 1;
        mock.subtitle_languages = vec![Some("fre".to_string())];
        mock.subtitle_titles = vec![Some("French".to_string())];
        assert_eq!(mock.subtitle_language(0), Some("fre".to_string()));
        assert_eq!(mock.subtitle_title(0), Some("French".to_string()));
    }
}
