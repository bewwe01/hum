//! The screens. Each file adds one `*_screen` method to `HumApp`.

mod beats;
mod looper;
pub use looper::layer_color;
mod play;
mod settings;
mod songs;
pub use songs::SongsUi;
mod sounds;
