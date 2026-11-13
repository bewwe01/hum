//! Wav writing (the encoder lives in hum-engine).

use std::io::{self, Write};
use std::path::Path;

pub use hum_engine::wav::encode;

pub fn write(path: &Path, samples: &[f32], channels: u16, sample_rate: u32) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir)?;
        }
    }
    let mut file = std::fs::File::create(path)?;
    file.write_all(&encode(samples, channels, sample_rate))
}
