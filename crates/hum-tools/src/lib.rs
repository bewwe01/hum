//! Offline rendering and measurement helpers for Hum's sounds.
//!
//! These exist because DSP cannot be judged by reading code: the `render`
//! binary writes wav files to listen to and prints measurements to compare.

pub mod analysis;
pub mod render;
pub mod wav;
