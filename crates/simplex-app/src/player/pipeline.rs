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

pub struct PlayerPipeline {
    pipeline: Element,
    paintable_sink: Element,
    pub picture: Picture,
    uri: Option<String>,
    _bus_watch_guard: Option<gst::bus::BusWatchGuard>,
    audio_streams: Arc<Mutex<Vec<gst::Stream>>>,
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

        Ok(Self {
            pipeline,
            paintable_sink,
            picture,
            uri: None,
            _bus_watch_guard: None,
            audio_streams: Arc::new(Mutex::new(Vec::new())),
        })
    }

    pub fn set_uri(&mut self, uri: &str) {
        self.uri = Some(uri.to_string());
        self.audio_streams.lock().unwrap().clear();
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

    /// Get the number of audio tracks.
    /// Falls back to 1 if the pipeline is playing but reports 0 tracks
    /// (common with HLS/adaptive streams).
    pub fn audio_track_count(&self) -> i32 {
        let count = self.try_i32_property("n-audio").unwrap_or(0);
        if count == 0 && self.is_playing() {
            tracing::debug!("n-audio is 0 while playing; assuming 1 audio track");
            1
        } else {
            count
        }
    }

    /// Get the current audio track index.
    pub fn current_audio_track(&self) -> i32 {
        self.try_i32_property("current-audio").unwrap_or(-1)
    }

    /// Set the current audio track by index.
    pub fn set_audio_track(&self, index: i32) {
        if self.pipeline.find_property("current-audio").is_some() {
            self.pipeline.set_property("current-audio", index);
        }
    }

    /// Get the number of subtitle tracks.
    pub fn subtitle_track_count(&self) -> i32 {
        self.try_i32_property("n-text").unwrap_or(0)
    }

    /// Get the current subtitle track index.
    pub fn current_subtitle_track(&self) -> i32 {
        self.try_i32_property("current-text").unwrap_or(-1)
    }

    /// Set the current subtitle track by index.
    pub fn set_subtitle_track(&self, index: i32) {
        if self.pipeline.find_property("current-text").is_some() {
            self.pipeline.set_property("current-text", index);
        }
    }

    /// Change playback speed using a rate-seek on the current position.
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

    /// Get the underlying GStreamer element for signal connections.
    pub fn element(&self) -> &Element {
        &self.pipeline
    }

    /// Get the tag list for a given audio track index.
    /// Tries the playbin2-style `get-audio-tags` action signal first, then
    /// falls back to the cached `StreamCollection` (required for playbin3).
    fn audio_tags(&self, index: i32) -> Option<gst::TagList> {
        if glib::subclass::SignalId::lookup("get-audio-tags", self.pipeline.type_()).is_some() {
            return self.pipeline.emit_by_name("get-audio-tags", &[&index]);
        }
        let streams = self.audio_streams.lock().unwrap();
        streams.get(index as usize).and_then(|s| s.tags())
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

    /// Get the paintable sink element.
    pub fn paintable_sink(&self) -> &Element {
        &self.paintable_sink
    }

    /// Safely read an i32 property, returning None if the property does not
    /// exist (e.g. pipeline not yet negotiated or in an error state).
    fn try_i32_property(&self, name: &str) -> Option<i32> {
        self.pipeline.find_property(name)?;
        Some(self.pipeline.property::<i32>(name))
    }

    /// True when the pipeline has reached at least PAUSED (streams
    /// have been negotiated and track properties are available).
    fn has_streams(&self) -> bool {
        let (_, current, _) = self.pipeline.state(gst::ClockTime::ZERO);
        matches!(current, State::Paused | State::Playing)
    }

    /// Connect to the bus for error/EOS/state-change handling.
    /// Also intercepts `StreamCollection` messages to cache audio stream
    /// metadata for playbin3 (which lacks the `get-audio-tags` signal).
    pub fn connect_bus<F: Fn(MessageView) + 'static>(&mut self, callback: F) {
        let bus = self.pipeline.bus().expect("Pipeline has no bus");
        let streams_cache = self.audio_streams.clone();
        let guard = bus.add_watch_local(move |_, msg| {
            if let MessageView::StreamCollection(sc) = msg.view() {
                let collection = sc.stream_collection();
                let mut audio = Vec::new();
                let n = collection.len() as u32;
                for i in 0..n {
                    if let Some(stream) = collection.stream(i) {
                        if stream.stream_type().contains(gst::StreamType::AUDIO) {
                            audio.push(stream);
                        }
                    }
                }
                tracing::debug!("StreamCollection: found {} audio stream(s)", audio.len());
                *streams_cache.lock().unwrap() = audio;
            }
            callback(msg.view());
            glib::ControlFlow::Continue
        })
        .expect("Failed to add bus watch");
        self._bus_watch_guard = Some(guard);
    }
}

impl Drop for PlayerPipeline {
    fn drop(&mut self) {
        let _ = self.pipeline.set_state(State::Null);
    }
}
