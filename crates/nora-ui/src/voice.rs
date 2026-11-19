// //// Neoffice — added file (no upstream equivalent): the microphone of the pane, to dictate a request to Nora.

//! Dictation. The person clicks the microphone, speaks, clicks again: what was said is recorded by the browser
//! (`getUserMedia` and `MediaRecorder`, where the page has them) and handed to the pane as a [`Recorded`] to be turned
//! into words by the server ([`crate::PanelAction::Dictate`], then `NoraSession::dictation`). Nothing leaves the page
//! until the recording is stopped, by the person or at its time limit, and the microphone is closed at that moment. A recording
//! the browser ended by itself (the device gone, the permission taken back) is not sent. A page that goes out of sight (another
//! tab, a minimised window, a frame its page hides) may draw no more frames, so nothing drawn could stop in time: the microphone
//! is let go at once and the recording dropped, unsent. So it is when the page is left (`pagehide`), and when the app no longer
//! draws the panel while its page is in sight (its pane closed, something else shown in its place): a panel that is not drawn
//! can neither show that it listens nor be stopped ([`Sight`]).
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
    /// The microphone was let go, nothing is kept, and there is nothing to say: the page went out of sight or was left, or the
    /// app no longer drew the panel ([`Sight`]).
    Away,
}

/// The longest recording: past it the browser's recorder is stopped for the person (a request, not a meeting).
const MAX_SECONDS: f64 = 120.0;
/// A recording shorter than this, or with no voice above [`QUIET`], is not sent: a transcription of silence is made of
/// words nobody said.
const MIN_SECONDS: f64 = 0.6;
const QUIET: f32 = 0.02;

/// Looks in a row at which the app drew no frame at all, though the watch asks for one at each of its looks, before an open
/// microphone is let go: an app that draws nothing shows nothing (it stopped, or the browser paints its frame no more, as
/// Chrome does with a frame of another origin scrolled out of view, whose page is not hidden for all that). While the browser
/// still asks for the microphone nothing is open, and they don't count ([`Sight::look_asking`]).
const IDLE_LOOKS: u32 = 4;

/// What the watch over the microphone knows of the frames the app drew, from the click that asked for it. A panel the app no
/// longer draws (its pane closed, something else shown in its place, the panel hidden in its `Ui`) can't show that it listens,
/// nor be stopped, nor reach its time limit: the watch then lets the microphone go. The frames are counted in egui's passes
/// (`Context::cumulative_pass_nr`): the panel notes each pass it is drawn in where it can be seen, and the watch, between
/// frames, looks whether the passes that ran since its last look drew it. A look at which no pass ran says nothing of the
/// panel (a frame may be on its way): only [`IDLE_LOOKS`] of them in a row do.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[cfg_attr(all(not(target_arch = "wasm32"), not(test)), allow(dead_code))]
struct Sight {
    /// The app's count of passes at the last look: the passes from this one on are not looked at yet.
    looked_at: u64,
    /// The last pass the panel was drawn in.
    drawn_in: u64,
    /// The looks in a row at which no pass had run at all.
    idle: u32,
}

#[cfg_attr(all(not(target_arch = "wasm32"), not(test)), allow(dead_code))]
impl Sight {
    /// The watch of a recording asked for in `pass` (the panel was drawn in it: its microphone button was clicked).
    fn new(pass: u64) -> Self {
        Sight { looked_at: pass.saturating_add(1), drawn_in: pass, idle: 0 }
    }

    /// The panel is drawn in `pass`.
    fn drawn(&mut self, pass: u64) {
        self.drawn_in = self.drawn_in.max(pass);
    }

    /// A look of the watch, the app's count of passes being `passes`: whether the microphone is to be let go.
    fn look(&mut self, passes: u64) -> bool {
        // The passes from `looked_at` to `passes - 1` ran since the last look; the panel was in one of them if it was drawn in
        // `looked_at` or later.
        let ran = passes > self.looked_at;
        let unseen = ran && self.drawn_in < self.looked_at;
        self.idle = if ran { 0 } else { self.idle.saturating_add(1) };
        self.looked_at = self.looked_at.max(passes);
        unseen || self.idle >= IDLE_LOOKS
    }

    /// A look made while the browser still asks for the microphone, or as it hands it over: whether the panel was left out of the
    /// passes that ran since the last look. Nothing is open yet that frames drawn no more could leave unseen (a browser may stop
    /// drawing behind its question), so the looks without a pass are not counted, and their count starts again.
    fn look_asking(&mut self, passes: u64) -> bool {
        let unseen = passes > self.looked_at && self.drawn_in < self.looked_at;
        self.idle = 0;
        self.looked_at = self.looked_at.max(passes);
        unseen
    }
}

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

    /// The panel is drawn in this pass of the app (`ctx`'s): while it is, the microphone it shows may stay open ([`Sight`]).
    pub fn drawn(&mut self, ctx: &egui::Context) {
        self.recorder.drawn(ctx);
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
                if other != Recorded::Away {
                    self.notice = Some(other.clone());
                }
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

    /// The server could not write the recording down.
    pub fn say_failed(&mut self) {
        self.notice = Some(Recorded::Failed);
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

    /// A look of the watch over the microphone, as the browser's makes between frames: whether it lets the microphone go.
    #[cfg(test)]
    pub fn look_for_test(&mut self, ctx: &egui::Context) -> bool {
        self.recorder.look(ctx.cumulative_pass_nr())
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod platform {
    use super::Recorded;
    #[cfg(test)]
    use super::Sight;

    /// No recorder outside a browser; the tests may hand it a recording, and look as the browser's watch does.
    #[derive(Default)]
    pub(super) struct Recorder {
        #[cfg(test)]
        ready: Option<Recorded>,
        #[cfg(test)]
        sight: Sight,
    }

    pub(super) fn available() -> bool {
        false
    }

    impl Recorder {
        pub fn start(&mut self, ctx: &egui::Context) {
            #[cfg(test)]
            {
                self.sight = Sight::new(ctx.cumulative_pass_nr());
            }
            let _ = ctx;
        }
        pub fn drawn(&mut self, ctx: &egui::Context) {
            #[cfg(test)]
            self.sight.drawn(ctx.cumulative_pass_nr());
            let _ = ctx;
        }
        /// A look of the watch: one that lets the microphone go leaves what the browser's leaves, a recording dropped.
        #[cfg(test)]
        pub fn look(&mut self, passes: u64) -> bool {
            let unseen = self.sight.look(passes);
            if unseen {
                self.ready = Some(Recorded::Away);
            }
            unseen
        }
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

    use super::{Recorded, Sight};

    /// The types the browsers record, best first: Chrome and Firefox speak webm/ogg opus, Safari mp4.
    const TYPES: [&str; 4] = ["audio/webm;codecs=opus", "audio/webm", "audio/ogg;codecs=opus", "audio/mp4"];
    /// The event of a page that goes out of sight or comes back.
    const VISIBILITY: &str = "visibilitychange";
    /// The event of a page that is left (closed, reloaded, navigated away from, or put in the browser's back-forward cache). Not
    /// `beforeunload`: a page may answer it by keeping the person (an editor with unsaved work asks), who then stays.
    const LEAVE: &str = "pagehide";
    /// How often the watch of a recording, from the click on, looks whether the panel is still drawn and its frame still shown,
    /// in milliseconds: a frame the page around it hides (`display: none`, as the desk does with a page it leaves) is told
    /// nothing, nor is a panel its app stopped drawing.
    const WATCH_MS: i32 = 500;

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
        /// The analyser ever read a sample that is not digital silence: a context that was not running (a browser that
        /// starts it only on a gesture) reads silence whatever is said, and then the quiet test says nothing.
        saw_signal: bool,
        listening: bool,
        /// The person gave the recording up: nothing is kept.
        cancelled: bool,
        /// What the recording came to, once it is over.
        result: Option<Recorded>,
        /// The closures the browser calls back, kept alive with the recording.
        keep: Vec<Closure<dyn FnMut(BlobEvent)>>,
        keep_stop: Vec<Closure<dyn FnMut()>>,
        /// What listens for the page going out of sight and for the page being left, while the microphone is open, and the
        /// timer that looks at the frame and at the panel, from the click on ([`watch`]).
        watch: Option<js_sys::Function>,
        leave: Option<js_sys::Function>,
        watch_timer: Option<i32>,
        /// The passes the panel was drawn in, as the watch knows them, and the app's viewport they are counted in.
        sight: Sight,
        viewport: egui::ViewportId,
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

    fn document() -> Option<web_sys::Document> {
        web_sys::window()?.document()
    }

    /// Whether the page is out of sight, or the frame it is in has no place in the page around it: it may draw no frames then.
    fn out_of_sight() -> bool {
        let hidden_frame =
            || web_sys::window().and_then(|window| window.frame_element().ok().flatten()).is_some_and(|frame| frame.get_client_rects().length() == 0);
        document().is_some_and(|page| page.hidden()) || hidden_frame()
    }

    fn stop_tracks(stream: &MediaStream) {
        let tracks = stream.get_tracks();
        for index in 0..tracks.length() {
            if let Ok(track) = tracks.get(index).dyn_into::<web_sys::MediaStreamTrack>() {
                track.stop();
            }
        }
    }

    fn release(shared: &mut Shared) {
        if let Some(stream) = shared.stream.take() {
            stop_tracks(&stream);
        }
        if let (Some(watch), Some(page)) = (shared.watch.take(), document()) {
            let _ = page.remove_event_listener_with_callback(VISIBILITY, &watch);
        }
        if let (Some(leave), Some(window)) = (shared.leave.take(), web_sys::window()) {
            let _ = window.remove_event_listener_with_callback(LEAVE, &leave);
        }
        if let (Some(timer), Some(window)) = (shared.watch_timer.take(), web_sys::window()) {
            window.clear_interval_with_handle(timer);
        }
        if let Some(audio) = shared.audio.take() {
            let _ = audio.close();
        }
        shared.analyser = None;
        // Its late events (the last data, the stop of a recording given up) reach nothing: their closures go at the next start.
        if let Some(recorder) = shared.recorder.take() {
            recorder.set_ondataavailable(None);
            recorder.set_onstop(None);
        }
        shared.listening = false;
    }

    /// The app's count of passes in the viewport the panel is drawn in.
    fn passes(shared: &Shared) -> u64 {
        shared.repaint.as_ref().map_or(0, |ctx| ctx.cumulative_pass_nr_for(shared.viewport))
    }

    /// The watch of run `epoch`, from the click on, between frames: the frame hidden by the page around it, or the panel no
    /// longer drawn by its app ([`Sight`]). Each look asks for a frame, so that an app with nothing else to draw draws one the
    /// next look can count. While the browser still asks for the microphone (no stream yet), only a panel left out of the
    /// passes lets the request go: the page out of sight then (the person answering from another tab) is looked at when the
    /// browser hands the microphone over. It stops with the run ([`release`]).
    fn watch(shared: &Rc<RefCell<Shared>>, epoch: u64) {
        let Some(window) = web_sys::window() else { return };
        let on_watch = {
            let shared = shared.clone();
            Closure::<dyn FnMut()>::new(move || {
                let (unseen, asking) = {
                    let Ok(mut state) = shared.try_borrow_mut() else { return };
                    if state.epoch != epoch {
                        return;
                    }
                    let passes = passes(&state);
                    let asking = state.stream.is_none();
                    let unseen = if asking { state.sight.look_asking(passes) } else { state.sight.look(passes) };
                    if let Some(ctx) = &state.repaint {
                        ctx.request_repaint();
                    }
                    (unseen, asking)
                };
                if unseen || (!asking && out_of_sight()) {
                    let_go(&shared, epoch);
                }
            })
        };
        let timer = window.set_interval_with_callback_and_timeout_and_arguments_0(on_watch.as_ref().unchecked_ref(), WATCH_MS).ok();
        let Ok(mut state) = shared.try_borrow_mut() else {
            if let Some(timer) = timer {
                window.clear_interval_with_handle(timer);
            }
            return;
        };
        state.watch_timer = timer;
        // Kept with the run's other callbacks: it may be running when the run ends.
        state.keep_stop.push(on_watch);
    }

    /// Lets the microphone of run `epoch` go, at once, and drops what was said: nothing drawn could stop it in time.
    fn let_go(shared: &Rc<RefCell<Shared>>, epoch: u64) {
        let Ok(mut state) = shared.try_borrow_mut() else { return };
        if state.epoch != epoch {
            return;
        }
        state.epoch += 1;
        state.cancelled = true;
        release(&mut state);
        state.result = Some(Recorded::Away);
        if let Some(ctx) = &state.repaint {
            ctx.request_repaint();
        }
    }

    impl Recorder {
        pub fn start(&mut self, ctx: &egui::Context) {
            let epoch = {
                let mut shared = self.shared.borrow_mut();
                release(&mut shared);
                *shared = Shared {
                    epoch: shared.epoch + 1,
                    repaint: Some(ctx.clone()),
                    sight: Sight::new(ctx.cumulative_pass_nr()),
                    viewport: ctx.viewport_id(),
                    ..Shared::default()
                };
                shared.epoch
            };
            watch(&self.shared, epoch);
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

        pub fn drawn(&mut self, ctx: &egui::Context) {
            if let Ok(mut shared) = self.shared.try_borrow_mut()
                && ctx.viewport_id() == shared.viewport
            {
                shared.sight.drawn(ctx.cumulative_pass_nr());
            }
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
                // Still asking for the microphone: the recording is given up (its watch with it), and the microphone closed when
                // it opens.
                None => {
                    shared.epoch += 1;
                    release(&mut shared);
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
            if samples.iter().any(|sample| *sample != 128) {
                shared.saw_signal = true;
            }
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
                // The level is only trusted where the analyser ever heard something: a room is never digital silence.
                if seconds < min_seconds || (shared.saw_signal && shared.peak < quiet) {
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
            stop_tracks(&stream);
            return Ok(());
        }
        // Granted while the page was out of sight (the person answered from elsewhere), or once the app drew the panel no more in
        // the passes since the watch last looked (its pane closed while the browser asked): nothing is recorded unseen. A pane
        // closed since that look is the watch's at its next one.
        let unseen = {
            let mut state = shared.borrow_mut();
            let passes = passes(&state);
            state.sight.look_asking(passes)
        };
        if out_of_sight() || unseen {
            stop_tracks(&stream);
            return Err(Recorded::Away);
        }
        let mime = TYPES.iter().find(|candidate| MediaRecorder::is_type_supported(candidate)).copied().unwrap_or("");
        let options = MediaRecorderOptions::new();
        if !mime.is_empty() {
            options.set_mime_type(mime);
        }
        // A recorder the browser refuses leaves a stream nobody holds: its microphone is closed here.
        let recorder = MediaRecorder::new_with_media_stream_and_media_recorder_options(&stream, &options).map_err(|_| {
            stop_tracks(&stream);
            Recorded::Failed
        })?;

        // The level of the voice, for the bars and for telling a silence from a request.
        let (audio, analyser) = match AudioContext::new() {
            Ok(audio) => match (audio.create_media_stream_source(&stream), audio.create_analyser()) {
                (Ok(source), Ok(analyser)) => {
                    // A browser that starts audio only on a gesture hands back a suspended context: the level would read nothing.
                    let _ = audio.resume();
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
                let (chunks, mime) = {
                    let mut state = shared.borrow_mut();
                    // The late stop of a recording given up: what runs now is another's, and stays open.
                    if state.epoch != epoch || state.cancelled {
                        return;
                    }
                    release(&mut state);
                    let chunks = std::mem::take(&mut state.chunks);
                    // Nobody asked for this stop: the browser ended the recording (the device went, the permission was taken
                    // back). What it holds was not meant to leave yet.
                    if state.stopped_at.is_none() {
                        state.result = Some(Recorded::Failed);
                        if let Some(ctx) = &state.repaint {
                            ctx.request_repaint();
                        }
                        return;
                    }
                    (chunks, state.mime.clone())
                };
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
        // Out of sight, the page may draw no frame that could stop the recording or reach its time limit: the microphone is let go
        // here, at once, and what was said is dropped.
        let on_hidden = {
            let shared = shared.clone();
            Closure::<dyn FnMut()>::new(move || {
                if out_of_sight() {
                    let_go(&shared, epoch);
                }
            })
        };
        // A page that is left takes nothing it heard with it, whatever the browser then does with the page.
        let on_leave = {
            let shared = shared.clone();
            Closure::<dyn FnMut()>::new(move || let_go(&shared, epoch))
        };
        let watch: js_sys::Function = on_hidden.as_ref().unchecked_ref::<js_sys::Function>().clone();
        if let Some(page) = document() {
            let _ = page.add_event_listener_with_callback(VISIBILITY, &watch);
        }
        let leave: js_sys::Function = on_leave.as_ref().unchecked_ref::<js_sys::Function>().clone();
        let _ = window.add_event_listener_with_callback(LEAVE, &leave);
        recorder.set_ondataavailable(Some(on_data.as_ref().unchecked_ref()));
        recorder.set_onstop(Some(on_stop.as_ref().unchecked_ref()));
        {
            let mut state = shared.borrow_mut();
            state.keep.push(on_data);
            state.keep_stop.push(on_stop);
            // Kept with the others: they may be running when the recording ends.
            state.keep_stop.push(on_hidden);
            state.keep_stop.push(on_leave);
            state.watch = Some(watch);
            state.leave = Some(leave);
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

    /// A panel drawn in the frames between two looks keeps its microphone, however many frames there were.
    #[test]
    fn a_panel_drawn_between_two_looks_keeps_the_microphone() {
        let mut sight = Sight::new(10);
        for pass in 11..40 {
            sight.drawn(pass);
            if pass % 7 == 0 {
                assert!(!sight.look(pass + 1), "drawn in pass {pass}");
            }
        }
        assert!(!sight.look(40));
    }

    /// Frames ran since the last look and the panel was in none of them: its pane closed, or something else is shown in its place.
    #[test]
    fn a_panel_left_out_of_the_frames_lets_the_microphone_go() {
        let mut sight = Sight::new(10);
        sight.drawn(11);
        assert!(!sight.look(12));
        // The pane closed after pass 11: passes 12 and 13 ran without it.
        assert!(sight.look(14));
        // Closed right after the click: the pass of the click drew it, the next ones did not.
        let mut sight = Sight::new(10);
        assert!(!sight.look(11), "nothing ran after the click yet");
        assert!(sight.look(13));
    }

    /// A look at which no frame ran says nothing of the panel (the app was busy, a frame is on its way); a few in a row mean the
    /// app draws nothing at all, and shows nothing.
    #[test]
    fn an_app_that_draws_no_frame_at_all_lets_the_microphone_go() {
        let mut sight = Sight::new(10);
        for _ in 1..IDLE_LOOKS {
            assert!(!sight.look(11));
        }
        assert!(sight.look(11));
        // A frame between the looks starts the count again.
        let mut sight = Sight::new(10);
        for _ in 1..IDLE_LOOKS {
            assert!(!sight.look(11));
        }
        sight.drawn(11);
        assert!(!sight.look(12));
        assert!(!sight.look(12));
    }

    /// While the browser asks for the microphone nothing is open: looks without a frame say nothing (a browser may stop drawing
    /// behind its question) and don't add up to the microphone just granted; a panel left out of the frames that ran does.
    #[test]
    fn while_the_browser_asks_only_a_panel_left_out_lets_go() {
        let mut sight = Sight::new(10);
        for _ in 0..IDLE_LOOKS * 2 {
            assert!(!sight.look_asking(11));
        }
        assert!(!sight.look(11), "the count of looks without a frame starts again once the microphone is open");
        let mut sight = Sight::new(10);
        sight.drawn(11);
        assert!(!sight.look_asking(12), "drawn while the browser asks");
        assert!(sight.look_asking(14), "the pane closed while the browser asks");
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
