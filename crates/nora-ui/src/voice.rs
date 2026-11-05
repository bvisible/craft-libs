// //// Neoffice — added file (no upstream equivalent): the microphone of the pane, to dictate a request to Nora.

//! Dictation. The person clicks the microphone, speaks, clicks again: what was said is recorded by the browser
//! (`getUserMedia` and `MediaRecorder`, where the page has them) and handed to the pane as a [`Recorded`] to be turned
//! into words by the server ([`crate::PanelAction::Dictate`], then `NoraSession::dictation`). Nothing leaves the page
//! until the person stopped the recording, and the microphone is closed the moment they do.
//!
//! The recorder is the browser's; outside a browser (the desktop build, the tests) there is none, and the pane offers no
//! microphone. The level of the voice (an `AnalyserNode`) moves the bars the pane draws while it listens.

/// What the pane says about the microphone, in the app's language. English by default.
#[derive(Debug, Clone)]
pub struct VoiceWords {
    /// The microphone's tip: « Dictate ».
    pub dictate: String,
    /// While it listens: the button that stops the recording and puts the words in the box.
    pub stop: String,
    /// Throws the recording away.
    pub cancel: String,
    /// While the server turns the recording into words.
    pub transcribing: String,
    /// A recording with nothing in it (too short, or silence).
    pub nothing_heard: String,
    /// The person (or the page) refused the microphone.
    pub denied: String,
    /// The computer has no microphone to open.
    pub no_microphone: String,
    /// The recording or its transcription failed.
    pub failed: String,
}

impl Default for VoiceWords {
    fn default() -> Self {
        VoiceWords {
            dictate: "Dictate".into(),
            stop: "Stop and keep what you said".into(),
            cancel: "Cancel".into(),
            transcribing: "Writing down what you said…".into(),
            nothing_heard: "I did not hear anything. Try again, a little closer to the microphone.".into(),
            denied: "The microphone is blocked. Allow it for this page, then try again.".into(),
            no_microphone: "No microphone found on this computer.".into(),
            failed: "The dictation did not work. Try again, or type your request.".into(),
        }
    }
}

/// A recording that is over. (The browser's refusals are only made in a browser.)
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) enum Recorded {
    /// What the browser recorded, and its type (`audio/webm`, `audio/mp4`…).
    Audio { bytes: Vec<u8>, mime: String },
    /// Too short, or nothing but silence.
    Nothing,
    /// The microphone was refused.
    Denied,
    /// There is no microphone.
    Missing,
    /// Anything else that went wrong.
    Failed,
}

/// The longest recording: past it the browser's recorder is stopped for the person (a request, not a meeting).
const MAX_SECONDS: f64 = 120.0;
/// A recording shorter than this, or with no voice above [`QUIET`], is not sent: a transcription of silence is made of
/// words nobody said.
const MIN_SECONDS: f64 = 0.6;
const QUIET: f32 = 0.02;

#[derive(Debug, Clone, Copy, PartialEq, Default)]
enum Phase {
    #[default]
    Idle,
    /// Asked the browser for the microphone, no answer yet.
    Opening,
    Listening {
        since: f64,
    },
    /// Stopped: the recording is being put together, then sent.
    Closing,
    /// Sent to the server, waiting for the words.
    Transcribing,
}

/// The state of the microphone of one pane.
#[derive(Default)]
pub(crate) struct Voice {
    phase: Phase,
    /// What to tell the person once, in a line under the composer.
    notice: Option<Recorded>,
    recorder: platform::Recorder,
    /// Whether the pane's tests pretend there is a microphone.
    #[cfg(test)]
    pretend: bool,
}

impl std::fmt::Debug for Voice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Voice").field("phase", &self.phase).finish()
    }
}

impl Voice {
    /// Whether this page can record.
    pub fn available(&self) -> bool {
        #[cfg(test)]
        if self.pretend {
            return true;
        }
        platform::available()
    }

    pub fn listening(&self) -> bool {
        matches!(self.phase, Phase::Opening | Phase::Listening { .. })
    }

    pub fn transcribing(&self) -> bool {
        matches!(self.phase, Phase::Closing | Phase::Transcribing)
    }

    /// Nothing under way: the microphone is closed and no words are awaited.
    pub fn settled(&self) -> bool {
        self.phase == Phase::Idle
    }

    /// Opens the microphone. The recording begins when the browser has it (the person may be asked first).
    pub fn start(&mut self, ctx: &egui::Context) {
        if self.phase != Phase::Idle {
            return;
        }
        self.notice = None;
        self.phase = Phase::Opening;
        self.recorder.start(ctx);
    }

    /// Stops the recording: it reaches [`Voice::take_recorded`] in a moment.
    pub fn stop(&mut self) {
        if self.listening() {
            self.phase = Phase::Closing;
            self.recorder.stop();
        }
    }

    /// Throws the recording away and closes the microphone.
    pub fn cancel(&mut self) {
        self.recorder.cancel();
        self.phase = Phase::Idle;
    }

    /// The voice's level, from 0 to 1, while it listens.
    pub fn level(&self) -> f32 {
        self.recorder.level()
    }

    /// How many seconds the recording has run.
    pub fn seconds(&self, now: f64) -> u64 {
        match self.phase {
            Phase::Listening { since } => (now - since).max(0.0) as u64,
            _ => 0,
        }
    }

    /// Looks at what the browser did since the last frame: the recording began (or was refused), or it is over. The
    /// recording that is over is returned once.
    pub fn poll(&mut self, now: f64) -> Option<Recorded> {
        if self.phase == Phase::Opening && self.recorder.is_listening() {
            self.phase = Phase::Listening { since: now };
        }
        if let Phase::Listening { since } = self.phase
            && now - since > MAX_SECONDS
        {
            self.stop();
        }
        let finished = self.recorder.take(MIN_SECONDS, QUIET)?;
        match finished {
            Recorded::Audio { .. } => self.phase = Phase::Transcribing,
            other => {
                self.phase = Phase::Idle;
                self.notice = Some(other.clone());
                return Some(other);
            }
        }
        Some(finished)
    }

    /// The words are in (or the transcription failed): the microphone is free again.
    pub fn finished(&mut self) {
        if self.transcribing() {
            self.phase = Phase::Idle;
        }
    }

    /// The server heard nothing in what was sent.
    pub fn say_nothing_heard(&mut self) {
        self.notice = Some(Recorded::Nothing);
    }

    /// The line to show once under the composer, if there is one.
    pub fn take_notice(&mut self) -> Option<Recorded> {
        self.notice.take()
    }

    #[cfg(test)]
    pub fn pretend_available(&mut self) {
        self.pretend = true;
    }

    /// A test's recording, as if the browser had ended one.
    #[cfg(test)]
    pub fn inject(&mut self, recorded: Recorded) {
        self.recorder.inject(recorded);
    }

    #[cfg(test)]
    pub fn begin_for_test(&mut self, now: f64) {
        self.phase = Phase::Listening { since: now };
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod platform {
    use super::Recorded;

    /// No recorder outside a browser; the tests may hand it a recording.
    #[derive(Default)]
    pub(super) struct Recorder {
        #[cfg(test)]
        ready: Option<Recorded>,
    }

    pub(super) fn available() -> bool {
        false
    }

    impl Recorder {
        pub fn start(&mut self, _ctx: &egui::Context) {}
        pub fn stop(&mut self) {}
        pub fn cancel(&mut self) {}
        pub fn level(&self) -> f32 {
            0.0
        }
        pub fn is_listening(&self) -> bool {
            false
        }
        #[cfg(test)]
        pub fn inject(&mut self, recorded: Recorded) {
            self.ready = Some(recorded);
        }
        #[cfg(test)]
        pub fn take(&mut self, _min_seconds: f64, _quiet: f32) -> Option<Recorded> {
            self.ready.take()
        }
        #[cfg(not(test))]
        pub fn take(&mut self, _min_seconds: f64, _quiet: f32) -> Option<Recorded> {
            None
        }
    }
}

#[cfg(target_arch = "wasm32")]
mod platform {
    use std::cell::RefCell;
    use std::rc::Rc;

    use js_sys::{Array, Reflect, Uint8Array};
    use wasm_bindgen::JsCast;
    use wasm_bindgen::prelude::*;
    use wasm_bindgen_futures::{JsFuture, spawn_local};
    use web_sys::{AnalyserNode, AudioContext, Blob, BlobEvent, MediaRecorder, MediaRecorderOptions, MediaStream, MediaStreamConstraints};

    use super::Recorded;

    /// The types the browsers record, best first: Chrome and Firefox speak webm/ogg opus, Safari mp4.
    const TYPES: [&str; 4] = ["audio/webm;codecs=opus", "audio/webm", "audio/ogg;codecs=opus", "audio/mp4"];

    pub(super) fn available() -> bool {
        let Some(window) = web_sys::window() else { return false };
        let has = |target: &JsValue, key: &str| {
            Reflect::get(target, &JsValue::from_str(key)).map(|value| !value.is_undefined() && !value.is_null()).unwrap_or(false)
        };
        // `navigator.mediaDevices` is missing on a page that is not served securely.
        has(&window, "MediaRecorder") && has(window.navigator().as_ref(), "mediaDevices")
    }

    #[derive(Default)]
    struct Shared {
        stream: Option<MediaStream>,
        recorder: Option<MediaRecorder>,
        audio: Option<AudioContext>,
        analyser: Option<AnalyserNode>,
        chunks: Vec<Blob>,
        mime: String,
        started: Option<f64>,
        stopped_at: Option<f64>,
        peak: f32,
        listening: bool,
        /// The person gave the recording up: nothing is kept.
        cancelled: bool,
        /// What the recording came to, once it is over.
        result: Option<Recorded>,
        /// The closures the browser calls back, kept alive with the recording.
        keep: Vec<Closure<dyn FnMut(BlobEvent)>>,
        keep_stop: Vec<Closure<dyn FnMut()>>,
        /// A run of this recorder: an older one's late callbacks do nothing.
        epoch: u64,
        repaint: Option<egui::Context>,
    }

    #[derive(Default)]
    pub(super) struct Recorder {
        shared: Rc<RefCell<Shared>>,
    }

    fn name_of(error: &JsValue) -> String {
        Reflect::get(error, &JsValue::from_str("name")).ok().and_then(|name| name.as_string()).unwrap_or_default()
    }

    fn release(shared: &mut Shared) {
        if let Some(stream) = shared.stream.take() {
            let tracks = stream.get_tracks();
            for index in 0..tracks.length() {
                if let Ok(track) = tracks.get(index).dyn_into::<web_sys::MediaStreamTrack>() {
                    track.stop();
                }
            }
        }
        if let Some(audio) = shared.audio.take() {
            let _ = audio.close();
        }
        shared.analyser = None;
        shared.recorder = None;
        shared.listening = false;
    }

    impl Recorder {
        pub fn start(&mut self, ctx: &egui::Context) {
            let epoch = {
                let mut shared = self.shared.borrow_mut();
                release(&mut shared);
                *shared = Shared { epoch: shared.epoch + 1, repaint: Some(ctx.clone()), ..Shared::default() };
                shared.epoch
            };
            let shared = self.shared.clone();
            spawn_local(async move {
                let outcome = open(&shared, epoch).await;
                if let Err(recorded) = outcome {
                    let mut state = shared.borrow_mut();
                    if state.epoch == epoch && !state.cancelled {
                        release(&mut state);
                        state.result = Some(recorded);
                    }
                }
                if let Some(ctx) = &shared.borrow().repaint {
                    ctx.request_repaint();
                }
            });
        }

        pub fn stop(&mut self) {
            let mut shared = self.shared.borrow_mut();
            shared.stopped_at = Some(js_sys::Date::now());
            match shared.recorder.clone() {
                Some(recorder) => {
                    if recorder.state() != web_sys::RecordingState::Inactive {
                        let _ = recorder.stop();
                    }
                }
                // Still asking for the microphone: the recording is given up, and the microphone closed when it opens.
                None => {
                    shared.epoch += 1;
                    shared.result = Some(Recorded::Nothing);
                }
            }
        }

        pub fn cancel(&mut self) {
            let mut shared = self.shared.borrow_mut();
            shared.cancelled = true;
            shared.epoch += 1;
            release(&mut shared);
            shared.result = None;
        }

        pub fn level(&self) -> f32 {
            let mut shared = self.shared.borrow_mut();
            let Some(analyser) = shared.analyser.clone() else { return 0.0 };
            let mut samples = [128u8; 256];
            analyser.get_byte_time_domain_data(&mut samples);
            let mean_square = samples.iter().map(|sample| ((*sample as f32 - 128.0) / 128.0).powi(2)).sum::<f32>() / samples.len() as f32;
            // A voice at talking volume reads 0.05 to 0.3: stretched, so that the bars use their height.
            let level = (mean_square.sqrt() * 4.0).clamp(0.0, 1.0);
            shared.peak = shared.peak.max(level);
            level
        }

        pub fn is_listening(&self) -> bool {
            self.shared.borrow().listening
        }

        pub fn take(&mut self, min_seconds: f64, quiet: f32) -> Option<Recorded> {
            let mut shared = self.shared.borrow_mut();
            let result = shared.result.take()?;
            if let Recorded::Audio { .. } = &result {
                let seconds = match (shared.started, shared.stopped_at) {
                    (Some(start), Some(end)) => (end - start) / 1000.0,
                    _ => min_seconds,
                };
                if seconds < min_seconds || shared.peak < quiet {
                    return Some(Recorded::Nothing);
                }
            }
            Some(result)
        }
    }

    /// Opens the microphone and starts the recorder. `Err` is what the person is to be told.
    async fn open(shared: &Rc<RefCell<Shared>>, epoch: u64) -> Result<(), Recorded> {
        let window = web_sys::window().ok_or(Recorded::Failed)?;
        let devices = window.navigator().media_devices().map_err(|_| Recorded::Failed)?;
        let constraints = MediaStreamConstraints::new();
        constraints.set_audio(&JsValue::TRUE);
        let promise = devices.get_user_media_with_constraints(&constraints).map_err(|_| Recorded::Failed)?;
        let stream: MediaStream = match JsFuture::from(promise).await {
            Ok(stream) => stream.unchecked_into(),
            Err(error) => {
                return Err(match name_of(&error).as_str() {
                    "NotAllowedError" | "SecurityError" | "PermissionDeniedError" => Recorded::Denied,
                    "NotFoundError" | "OverconstrainedError" | "DevicesNotFoundError" => Recorded::Missing,
                    _ => Recorded::Failed,
                });
            }
        };
        if shared.borrow().epoch != epoch || shared.borrow().cancelled {
            for index in 0..stream.get_tracks().length() {
                if let Ok(track) = stream.get_tracks().get(index).dyn_into::<web_sys::MediaStreamTrack>() {
                    track.stop();
                }
            }
            return Ok(());
        }
        let mime = TYPES.iter().find(|candidate| MediaRecorder::is_type_supported(candidate)).copied().unwrap_or("");
        let options = MediaRecorderOptions::new();
        if !mime.is_empty() {
            options.set_mime_type(mime);
        }
        let recorder = MediaRecorder::new_with_media_stream_and_media_recorder_options(&stream, &options).map_err(|_| Recorded::Failed)?;

        // The level of the voice, for the bars and for telling a silence from a request.
        let (audio, analyser) = match AudioContext::new() {
            Ok(audio) => match (audio.create_media_stream_source(&stream), audio.create_analyser()) {
                (Ok(source), Ok(analyser)) => {
                    analyser.set_fft_size(512);
                    let _ = source.connect_with_audio_node(&analyser);
                    (Some(audio), Some(analyser))
                }
                _ => (Some(audio), None),
            },
            Err(_) => (None, None),
        };

        let on_data = {
            let shared = shared.clone();
            Closure::<dyn FnMut(BlobEvent)>::new(move |event: BlobEvent| {
                if let Some(blob) = event.data()
                    && blob.size() > 0.0
                    && shared.borrow().epoch == epoch
                {
                    shared.borrow_mut().chunks.push(blob);
                }
            })
        };
        let on_stop = {
            let shared = shared.clone();
            Closure::<dyn FnMut()>::new(move || {
                let (chunks, mime, cancelled, current) = {
                    let mut state = shared.borrow_mut();
                    let current = state.epoch == epoch;
                    let mime = state.mime.clone();
                    let chunks = std::mem::take(&mut state.chunks);
                    let cancelled = state.cancelled;
                    release(&mut state);
                    (chunks, mime, cancelled, current)
                };
                if !current || cancelled {
                    return;
                }
                let shared = shared.clone();
                spawn_local(async move {
                    let parts = Array::new();
                    for chunk in &chunks {
                        parts.push(chunk);
                    }
                    let bag = web_sys::BlobPropertyBag::new();
                    bag.set_type(&mime);
                    let result = match Blob::new_with_blob_sequence_and_options(&parts, &bag) {
                        Ok(blob) => match JsFuture::from(blob.array_buffer()).await {
                            Ok(buffer) => {
                                Recorded::Audio { bytes: Uint8Array::new(&buffer).to_vec(), mime: mime.split(';').next().unwrap_or("").to_string() }
                            }
                            Err(_) => Recorded::Failed,
                        },
                        Err(_) => Recorded::Failed,
                    };
                    let mut state = shared.borrow_mut();
                    if state.epoch == epoch && !state.cancelled {
                        state.result = Some(result);
                    }
                    if let Some(ctx) = &state.repaint {
                        ctx.request_repaint();
                    }
                });
            })
        };
        recorder.set_ondataavailable(Some(on_data.as_ref().unchecked_ref()));
        recorder.set_onstop(Some(on_stop.as_ref().unchecked_ref()));
        {
            let mut state = shared.borrow_mut();
            state.keep.push(on_data);
            state.keep_stop.push(on_stop);
            state.mime = if mime.is_empty() { recorder.mime_type() } else { mime.to_string() };
            state.stream = Some(stream);
            state.audio = audio;
            state.analyser = analyser;
            state.recorder = Some(recorder.clone());
            state.started = Some(js_sys::Date::now());
            state.listening = true;
        }
        recorder.start().map_err(|_| Recorded::Failed)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_without_a_recorder_offers_no_microphone() {
        assert!(!Voice::default().available());
        let mut voice = Voice::default();
        voice.pretend_available();
        assert!(voice.available());
    }

    #[test]
    fn a_recording_goes_from_listening_to_transcribing_and_back() {
        let ctx = egui::Context::default();
        let mut voice = Voice::default();
        voice.pretend_available();
        assert!(voice.settled());
        voice.start(&ctx);
        assert!(voice.listening() && !voice.settled());
        voice.begin_for_test(10.0);
        assert_eq!(voice.seconds(14.0), 4);
        voice.stop();
        assert!(voice.transcribing());
        voice.inject(Recorded::Audio { bytes: vec![1, 2, 3], mime: "audio/webm".into() });
        assert!(matches!(voice.poll(15.0), Some(Recorded::Audio { .. })));
        assert!(voice.transcribing(), "waiting for the words");
        voice.finished();
        assert!(voice.settled());
    }

    #[test]
    fn a_silence_ends_the_recording_with_a_notice_and_nothing_to_send() {
        let ctx = egui::Context::default();
        let mut voice = Voice::default();
        voice.start(&ctx);
        voice.begin_for_test(0.0);
        voice.stop();
        voice.inject(Recorded::Nothing);
        assert_eq!(voice.poll(1.0), Some(Recorded::Nothing));
        assert!(voice.settled());
        assert_eq!(voice.take_notice(), Some(Recorded::Nothing));
        assert_eq!(voice.take_notice(), None, "said once");
    }

    #[test]
    fn cancel_closes_the_microphone_and_keeps_nothing() {
        let ctx = egui::Context::default();
        let mut voice = Voice::default();
        voice.start(&ctx);
        voice.begin_for_test(0.0);
        voice.cancel();
        assert!(voice.settled());
        assert_eq!(voice.poll(1.0), None);
    }
}
