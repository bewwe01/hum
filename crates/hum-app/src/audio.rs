//! Audio output: owns the cpal stream and connects the UI to the engine.
//!
//! Real-time rules for the callback: no allocation, no locks, no I/O.
//! * Notes and other discrete changes travel UI -> audio through a lock-free
//!   SPSC queue of [`Cmd`]s.
//! * Continuous controls and status are atomics in [`Shared`].
//! * Loop-timeline events travel audio -> UI through a second SPSC queue.
//!
//! The engine lives inside the callback. When the stream is closed (app in
//! the background, headphones unplugged, buffer setting changed) the engine
//! is handed back through a channel as the callback is dropped, so loops and
//! beats survive a stream restart.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use eframe::egui;
use hum_engine::engine::{Cmd, Engine, EngineEvent, Shared};
use serde::{Deserialize, Serialize};

const CMD_QUEUE: usize = 1024;
const EVENT_QUEUE: usize = 8192;
const PREFERRED_RATE: u32 = 48_000;
/// Frames rendered per engine call inside the callback.
const CHUNK: usize = 256;

/// The "Audio buffer" setting.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum BufferMode {
    /// Let the system pick (AAudio tunes the buffer itself).
    #[default]
    Auto,
    /// Smallest buffer: most responsive, may crackle.
    Low,
    /// Large buffer: no crackling, a little less responsive.
    Safe,
}

impl BufferMode {
    pub const ALL: [BufferMode; 3] = [BufferMode::Auto, BufferMode::Low, BufferMode::Safe];

    pub fn label(self) -> &'static str {
        match self {
            BufferMode::Auto => "Auto",
            BufferMode::Low => "Low",
            BufferMode::Safe => "Safe",
        }
    }

    fn buffer_size(self) -> cpal::BufferSize {
        match self {
            BufferMode::Auto => cpal::BufferSize::Default,
            BufferMode::Low => cpal::BufferSize::Fixed(96),
            BufferMode::Safe => cpal::BufferSize::Fixed(960),
        }
    }
}

/// Facts about the running stream, written by the audio thread.
#[derive(Debug, Default)]
pub struct StreamInfo {
    pub sample_rate: AtomicU32,
    /// Frames in the most recent callback.
    pub callback_frames: AtomicU32,
    /// Smoothed output latency in microseconds (0 = not known yet).
    pub latency_us: AtomicU32,
    pub xruns: AtomicU32,
    /// Set by the error callback when the stream must be reopened.
    pub broken: AtomicBool,
}

struct EngineSlot {
    engine: Engine,
    cmds: rtrb::Consumer<Cmd>,
    events: rtrb::Producer<EngineEvent>,
}

/// Returns the engine to the UI thread when the audio callback is dropped.
struct SlotGuard {
    slot: Option<Box<EngineSlot>>,
    home: mpsc::Sender<Box<EngineSlot>>,
}

impl Drop for SlotGuard {
    fn drop(&mut self) {
        if let Some(slot) = self.slot.take() {
            let _ = self.home.send(slot);
        }
    }
}

pub struct AudioHost {
    pub shared: Arc<Shared>,
    pub info: Arc<StreamInfo>,
    cmds: rtrb::Producer<Cmd>,
    events: rtrb::Consumer<EngineEvent>,
    home_tx: mpsc::Sender<Box<EngineSlot>>,
    home_rx: mpsc::Receiver<Box<EngineSlot>>,
    parked: Option<Box<EngineSlot>>,
    stream: Option<cpal::Stream>,
    /// Desktop only: a silent stand-in when there is no audio device.
    #[cfg(not(target_os = "android"))]
    null: Option<NullOutput>,
    mode: BufferMode,
    repaint: egui::Context,
    /// True when a brand-new engine was created; the UI must re-send its
    /// state and forget its loop mirror.
    engine_reset: bool,
    logged_stream: bool,
    last_error: Option<String>,
}

fn new_slot(
    sample_rate: f32,
    shared: &Arc<Shared>,
) -> (EngineSlot, rtrb::Producer<Cmd>, rtrb::Consumer<EngineEvent>) {
    let (cmd_tx, cmd_rx) = rtrb::RingBuffer::new(CMD_QUEUE);
    let (event_tx, event_rx) = rtrb::RingBuffer::new(EVENT_QUEUE);
    let slot = EngineSlot {
        engine: Engine::new(sample_rate, shared.clone()),
        cmds: cmd_rx,
        events: event_tx,
    };
    (slot, cmd_tx, event_rx)
}

impl AudioHost {
    pub fn new(repaint: egui::Context) -> Self {
        let shared = Arc::new(Shared::default());
        let (slot, cmds, events) = new_slot(PREFERRED_RATE as f32, &shared);
        let (home_tx, home_rx) = mpsc::channel();
        Self {
            shared,
            info: Arc::new(StreamInfo::default()),
            cmds,
            events,
            home_tx,
            home_rx,
            parked: Some(Box::new(slot)),
            stream: None,
            #[cfg(not(target_os = "android"))]
            null: None,
            mode: BufferMode::Auto,
            repaint,
            engine_reset: true,
            logged_stream: false,
            last_error: None,
        }
    }

    pub fn is_running(&self) -> bool {
        self.stream.is_some() || self.is_silent()
    }

    /// True when the engine runs without an audio device (desktop only).
    pub fn is_silent(&self) -> bool {
        #[cfg(not(target_os = "android"))]
        return self.null.is_some();
        #[cfg(target_os = "android")]
        false
    }

    pub fn mode(&self) -> BufferMode {
        self.mode
    }

    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    /// Queue a command for the engine. Never blocks.
    pub fn send(&mut self, cmd: Cmd) {
        if self.cmds.push(cmd).is_err() {
            log::warn!("audio command queue is full; dropped {cmd:?}");
        }
    }

    /// Next loop-timeline event from the engine, if any.
    pub fn poll_event(&mut self) -> Option<EngineEvent> {
        self.events.pop().ok()
    }

    /// True once after a fresh engine was created.
    pub fn take_engine_reset(&mut self) -> bool {
        std::mem::take(&mut self.engine_reset)
    }

    /// True when the error callback asked for the stream to be reopened.
    pub fn needs_reopen(&self) -> bool {
        self.info.broken.load(Ordering::Relaxed)
    }

    /// Measured output latency in milliseconds, once known.
    pub fn latency_ms(&self) -> Option<f32> {
        match self.info.latency_us.load(Ordering::Relaxed) {
            0 => None,
            us => Some(us as f32 / 1000.0),
        }
    }

    /// Log the stream parameters once the first callback has run.
    pub fn log_stream_once(&mut self) {
        if self.logged_stream || self.stream.is_none() {
            return;
        }
        let frames = self.info.callback_frames.load(Ordering::Relaxed);
        if frames == 0 {
            return;
        }
        self.logged_stream = true;
        log::info!(
            "audio stream: {} Hz, {} frames per callback, buffer mode {:?}, latency {}",
            self.info.sample_rate.load(Ordering::Relaxed),
            frames,
            self.mode,
            self.latency_ms()
                .map_or("unknown".to_owned(), |ms| format!("{ms:.1} ms")),
        );
    }

    pub fn set_mode(&mut self, mode: BufferMode) {
        if mode != self.mode {
            self.mode = mode;
            if self.is_running() {
                self.stop();
                self.start();
            }
        }
    }

    /// Close the stream and take the engine back.
    pub fn stop(&mut self) {
        #[cfg(not(target_os = "android"))]
        if let Some(null) = self.null.take() {
            self.send(Cmd::AllNotesOff);
            drop(null);
        }
        if let Some(stream) = self.stream.take() {
            // Nothing should be left ringing when the stream comes back.
            self.send(Cmd::AllNotesOff);
            let _ = stream.pause();
            drop(stream);
            log::info!("audio stream closed");
        }
        self.info.broken.store(false, Ordering::Relaxed);
        self.info.callback_frames.store(0, Ordering::Relaxed);
        self.info.latency_us.store(0, Ordering::Relaxed);
        self.logged_stream = false;
    }

    fn reclaim_engine(&mut self) {
        if self.parked.is_some() {
            return;
        }
        // The callback (and with it the engine) is dropped when the stream
        // closes; give the backend a moment if it does that asynchronously.
        if let Ok(slot) = self.home_rx.recv_timeout(Duration::from_millis(400)) {
            self.parked = Some(slot);
        }
    }

    /// Open the output stream (no-op if it is already open).
    pub fn start(&mut self) {
        if self.is_running() {
            return;
        }
        self.info.broken.store(false, Ordering::Relaxed);
        self.reclaim_engine();
        match self.open() {
            Ok(stream) => {
                self.stream = Some(stream);
                self.last_error = None;
            }
            Err(err) => {
                log::error!("could not open the audio output: {err}");
                self.last_error = Some(err);
                // On a desktop without sound hardware, keep the engine (and
                // with it the beat and loop clocks) running silently so the
                // UI can still be developed.
                #[cfg(not(target_os = "android"))]
                {
                    let slot = self.take_slot(PREFERRED_RATE as f32);
                    self.null = Some(NullOutput::start(
                        slot,
                        self.home_tx.clone(),
                        self.info.clone(),
                    ));
                    self.info
                        .sample_rate
                        .store(PREFERRED_RATE, Ordering::Relaxed);
                    log::warn!("no audio device: running the engine silently");
                }
            }
        }
    }

    /// Reopen after a route change (headphones plugged or unplugged).
    pub fn reopen(&mut self) {
        log::info!("reopening the audio stream");
        self.stop();
        self.start();
    }

    fn open(&mut self) -> Result<cpal::Stream, String> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or("no audio output device")?;
        let default = device.default_output_config().map_err(|e| e.to_string())?;
        let format = default.sample_format();

        // Preferred: 48 kHz stereo with the chosen buffer. Fallbacks: the
        // system's buffer size, then the device's own default format.
        let attempts = [
            (PREFERRED_RATE, 2u16, self.mode.buffer_size()),
            (PREFERRED_RATE, 2u16, cpal::BufferSize::Default),
            (
                default.sample_rate(),
                default.channels(),
                cpal::BufferSize::Default,
            ),
        ];
        let mut last_err = String::from("no stream configuration worked");
        for (rate, channels, buffer_size) in attempts {
            let config = cpal::StreamConfig {
                channels,
                sample_rate: rate,
                buffer_size,
            };
            let slot = self.take_slot(rate as f32);
            let result = match format {
                cpal::SampleFormat::I16 => self.build::<i16>(&device, config, slot),
                cpal::SampleFormat::U16 => self.build::<u16>(&device, config, slot),
                _ => self.build::<f32>(&device, config, slot),
            };
            match result {
                Ok(stream) => {
                    if let Err(err) = stream.play() {
                        last_err = err.to_string();
                        drop(stream);
                        self.reclaim_engine();
                        continue;
                    }
                    self.info.sample_rate.store(rate, Ordering::Relaxed);
                    log::info!(
                        "audio stream opened: requested {rate} Hz, {channels} ch, {format:?}, buffer {buffer_size:?}"
                    );
                    return Ok(stream);
                }
                Err(err) => {
                    log::warn!(
                        "audio config {rate} Hz / {channels} ch / {buffer_size:?} failed: {err}"
                    );
                    last_err = err;
                    self.reclaim_engine();
                }
            }
        }
        Err(last_err)
    }

    /// Take the parked engine, or make a new one if it is missing or was
    /// built for a different sample rate.
    fn take_slot(&mut self, sample_rate: f32) -> Box<EngineSlot> {
        match self.parked.take() {
            Some(slot) if (slot.engine.sample_rate() - sample_rate).abs() < 0.5 => slot,
            _ => {
                let (slot, cmds, events) = new_slot(sample_rate, &self.shared);
                self.cmds = cmds;
                self.events = events;
                self.engine_reset = true;
                Box::new(slot)
            }
        }
    }

    fn build<T>(
        &mut self,
        device: &cpal::Device,
        config: cpal::StreamConfig,
        slot: Box<EngineSlot>,
    ) -> Result<cpal::Stream, String>
    where
        T: cpal::SizedSample + cpal::FromSample<f32>,
    {
        let channels = usize::from(config.channels.max(1));
        let info = self.info.clone();
        let mut guard = SlotGuard {
            slot: Some(slot),
            home: self.home_tx.clone(),
        };
        let mut scratch = [0.0f32; CHUNK * 2];

        let data_callback = move |data: &mut [T], cb: &cpal::OutputCallbackInfo| {
            flush_denormals();
            let Some(slot) = guard.slot.as_mut() else {
                return;
            };
            while let Ok(cmd) = slot.cmds.pop() {
                slot.engine.handle(cmd);
            }
            let frames = data.len() / channels;
            let mut done = 0;
            while done < frames {
                let n = (frames - done).min(CHUNK);
                let block = &mut scratch[..n * 2];
                slot.engine.process(block);
                for (i, frame) in data[done * channels..(done + n) * channels]
                    .chunks_exact_mut(channels)
                    .enumerate()
                {
                    let (l, r) = (block[i * 2], block[i * 2 + 1]);
                    if channels == 1 {
                        frame[0] = T::from_sample(0.5 * (l + r));
                    } else {
                        frame[0] = T::from_sample(l);
                        frame[1] = T::from_sample(r);
                        for extra in frame[2..].iter_mut() {
                            *extra = T::EQUILIBRIUM;
                        }
                    }
                }
                done += n;
            }
            let events = &mut slot.events;
            slot.engine.drain_events(|event| {
                let _ = events.push(event);
            });

            info.callback_frames.store(frames as u32, Ordering::Relaxed);
            let ts = cb.timestamp();
            let latency = ts
                .playback
                .duration_since(ts.callback)
                .as_micros()
                .min(2_000_000) as u32;
            if latency > 0 {
                let old = info.latency_us.load(Ordering::Relaxed);
                let smoothed = if old == 0 {
                    latency
                } else {
                    old - old / 8 + latency / 8
                };
                info.latency_us.store(smoothed.max(1), Ordering::Relaxed);
            }
        };

        let info = self.info.clone();
        let repaint = self.repaint.clone();
        let error_callback = move |err: cpal::Error| match err.kind() {
            cpal::ErrorKind::Xrun => {
                info.xruns.fetch_add(1, Ordering::Relaxed);
            }
            cpal::ErrorKind::RealtimeDenied => {
                log::warn!("audio thread did not get real-time priority: {err}");
            }
            _ => {
                // Typically a route change (headphones); the UI thread
                // reopens the stream.
                log::warn!("audio stream error: {err}");
                info.broken.store(true, Ordering::Relaxed);
                repaint.request_repaint();
            }
        };

        device
            .build_output_stream(config, data_callback, error_callback, None)
            .map_err(|e| e.to_string())
    }
}

/// Desktop fallback: renders the engine in real time and throws the audio
/// away. Never used on Android.
#[cfg(not(target_os = "android"))]
struct NullOutput {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

#[cfg(not(target_os = "android"))]
impl NullOutput {
    fn start(
        slot: Box<EngineSlot>,
        home: mpsc::Sender<Box<EngineSlot>>,
        info: Arc<StreamInfo>,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let thread = std::thread::spawn(move || {
            let mut guard = SlotGuard {
                slot: Some(slot),
                home,
            };
            let mut block = [0.0f32; 480 * 2];
            let start = std::time::Instant::now();
            let mut rendered = 0u64;
            while !flag.load(Ordering::Relaxed) {
                let Some(slot) = guard.slot.as_mut() else {
                    break;
                };
                while let Ok(cmd) = slot.cmds.pop() {
                    slot.engine.handle(cmd);
                }
                // Keep pace with the wall clock.
                let due = start.elapsed().as_secs_f64() * f64::from(PREFERRED_RATE);
                while (rendered as f64) < due {
                    slot.engine.process(&mut block);
                    rendered += 480;
                }
                let events = &mut slot.events;
                slot.engine.drain_events(|event| {
                    let _ = events.push(event);
                });
                info.callback_frames.store(480, Ordering::Relaxed);
                std::thread::sleep(Duration::from_millis(10));
            }
        });
        Self {
            stop,
            thread: Some(thread),
        }
    }
}

#[cfg(not(target_os = "android"))]
impl Drop for NullOutput {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Make the CPU treat denormal floats as zero on the audio thread.
#[inline]
fn flush_denormals() {
    #[cfg(target_arch = "aarch64")]
    // SAFETY: only sets the FZ (flush-to-zero) bit of this thread's FPCR.
    unsafe {
        let mut fpcr: u64;
        core::arch::asm!("mrs {0}, fpcr", out(reg) fpcr);
        if fpcr & (1 << 24) == 0 {
            fpcr |= 1 << 24;
            core::arch::asm!("msr fpcr, {0}", in(reg) fpcr);
        }
    }
}
