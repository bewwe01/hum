//! Desktop dev entry point: `cargo run -p hum-app` opens a 390x844 window
//! where the mouse acts as a finger.

fn main() {
    #[cfg(not(target_os = "android"))]
    hum_app::desktop_main();
}
