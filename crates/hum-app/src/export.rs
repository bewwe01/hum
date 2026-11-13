//! Export the loop as a WAV file: an offline engine renders two passes of
//! the loop (plus the release tail) on a background thread, then the
//! platform saves it to Downloads/Hum.

use std::sync::mpsc;
use std::sync::Arc;

use hum_engine::engine::{Cmd, Engine, Shared};

use crate::app::HumApp;

const RATE: u32 = 48_000;
const PASSES: usize = 2;
const MAX_TAIL_SECONDS: usize = 6;

/// File name like `hum-loop-2026-11-13-1912.wav` (UTC).
fn file_name() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs()) as i64;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "hum-loop-{year:04}-{month:02}-{day:02}-{:02}{:02}.wav",
        rem / 3600,
        rem % 3600 / 60
    )
}

impl HumApp {
    pub(crate) fn export_loop(&mut self) {
        if self.exporting.is_some() {
            return;
        }
        let Some(looper) = &self.st.looper else {
            self.show_toast("Make a loop first, then export it");
            return;
        };
        let steps = looper.steps;
        let mut cmds = vec![Cmd::SetBpm(self.st.bpm), Cmd::SetLoopSteps(steps)];
        for slot in 0..looper.layers.len() {
            cmds.extend(self.layer_cmds(slot));
        }
        cmds.push(Cmd::LoopPlay(true));
        let loop_seconds = f64::from(steps) / 4.0 * 60.0 / f64::from(self.st.bpm);
        let soundfont = self.with_audio(|a| a.soundfont()).flatten();
        let volume = self.st.volume;
        let platform = self.platform.clone();
        let (tx, rx) = mpsc::channel();
        let wake = self.repaint.clone();
        let spawned = std::thread::Builder::new()
            .name("hum-export".into())
            .spawn(move || {
                let shared = Arc::new(Shared::default());
                shared.volume.set(volume);
                shared.reverb.set(0.0);
                let mut engine = Engine::new(RATE as f32, shared);
                if let Some(sf) = soundfont {
                    let settings = rustysynth::SynthesizerSettings::new(RATE as i32);
                    if let Ok(synth) = rustysynth::Synthesizer::new(&sf, &settings) {
                        engine.set_synth(Box::new(synth));
                    }
                }
                for cmd in cmds {
                    engine.handle(cmd);
                }
                let frames = (loop_seconds * f64::from(RATE)).round() as usize * PASSES;
                let mut samples = vec![0.0f32; frames * 2];
                for chunk in samples.chunks_mut(1024) {
                    engine.process(chunk);
                }
                // Let the last notes, delay and reverb ring out.
                engine.handle(Cmd::LoopPlay(false));
                let mut block = [0.0f32; 1024];
                let mut quiet = 0usize;
                for _ in 0..(MAX_TAIL_SECONDS * RATE as usize / 512) {
                    engine.process(&mut block);
                    samples.extend_from_slice(&block);
                    let loud = block.iter().any(|s| s.abs() > 1.0e-4);
                    quiet = if loud { 0 } else { quiet + 512 };
                    if quiet > RATE as usize / 4 {
                        break;
                    }
                }
                let bytes = hum_engine::wav::encode(&samples, 2, RATE);
                let result = platform.save_download(&file_name(), &bytes);
                let _ = tx.send(result);
                wake.request_repaint();
            });
        match spawned {
            Ok(_) => {
                self.exporting = Some(rx);
                self.show_toast("Exporting the loop…");
            }
            Err(err) => self.show_toast(&format!("Export failed: {err}")),
        }
    }

    /// Report a finished export.
    pub(crate) fn poll_export(&mut self) {
        let Some(rx) = &self.exporting else { return };
        match rx.try_recv() {
            Ok(Ok(path)) => {
                self.exporting = None;
                self.show_toast(&format!("Saved {path}"));
            }
            Ok(Err(err)) => {
                self.exporting = None;
                self.show_toast(&format!("Export failed: {err}"));
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                self.exporting = None;
                self.show_toast("Export failed");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn file_names_are_dated() {
        let name = super::file_name();
        assert!(
            name.starts_with("hum-loop-20") && name.ends_with(".wav"),
            "{name}"
        );
        assert_eq!(name.len(), "hum-loop-2026-11-13-1912.wav".len());
    }
}
