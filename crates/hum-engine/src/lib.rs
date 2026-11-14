//! Hum's synth engine: pure DSP with no platform dependencies.
//!
//! * [`engine::Engine`] renders audio and runs the beat sequencer, looper
//!   and arpeggiator. Its `process` call is real-time safe (no allocation,
//!   locks or I/O).
//! * [`presets::PRESETS`] is the table of the 56 built-in sounds.
//! * [`scale`] maps the seven on-screen keys to notes.

pub mod chords;
pub mod dsp;
pub mod engine;
pub mod fx;
pub mod preset;
pub mod presets;
pub mod scale;
pub mod transport;
pub mod voice;
pub mod wav;

pub use engine::{CaptureError, Cmd, Engine, EngineEvent, Shared, Status};
pub use preset::{Category, Preset, Wave};
pub use presets::PRESETS;
pub use scale::{KeyMap, Scale};
