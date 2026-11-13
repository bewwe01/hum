//! Logs where the UI thread is if a frame stops making progress, so a hang
//! on a phone shows up in logcat as "UI stuck in <stage>" instead of a
//! silent black screen.

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Stage {
    /// Between frames (idle, or inside eframe / the GPU driver).
    Outside = 0,
    Input = 1,
    Audio = 2,
    Insets = 3,
    Draw = 4,
    Save = 5,
}

impl Stage {
    fn from_u8(v: u8) -> Stage {
        match v {
            1 => Stage::Input,
            2 => Stage::Audio,
            3 => Stage::Insets,
            4 => Stage::Draw,
            5 => Stage::Save,
            _ => Stage::Outside,
        }
    }
}

#[derive(Clone, Default)]
pub struct Watchdog {
    stage: Arc<AtomicU8>,
    ticks: Arc<AtomicU64>,
    foreground: Arc<AtomicBool>,
    started: Arc<AtomicBool>,
}

impl Watchdog {
    pub fn start(&self) {
        if self.started.swap(true, Ordering::Relaxed) {
            return;
        }
        self.foreground.store(true, Ordering::Relaxed);
        let me = self.clone();
        let _ = std::thread::Builder::new()
            .name("hum-watchdog".into())
            .spawn(move || {
                let mut last = u64::MAX;
                let mut stuck_for = 0u32;
                loop {
                    std::thread::sleep(Duration::from_secs(1));
                    let ticks = me.ticks.load(Ordering::Relaxed);
                    let stage = Stage::from_u8(me.stage.load(Ordering::Relaxed));
                    if ticks == last
                        && stage != Stage::Outside
                        && me.foreground.load(Ordering::Relaxed)
                    {
                        stuck_for += 1;
                        if stuck_for == 3 || stuck_for.is_multiple_of(10) {
                            log::error!("watchdog: UI thread stuck in {stage:?} for {stuck_for} s");
                        }
                    } else {
                        stuck_for = 0;
                    }
                    last = ticks;
                }
            });
    }

    pub fn enter(&self, stage: Stage) {
        self.stage.store(stage as u8, Ordering::Relaxed);
        self.ticks.fetch_add(1, Ordering::Relaxed);
    }

    pub fn set_foreground(&self, foreground: bool) {
        self.foreground.store(foreground, Ordering::Relaxed);
    }
}
