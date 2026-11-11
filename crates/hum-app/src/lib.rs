//! Hum: UI and audio glue. The synth itself lives in `hum-engine`.

mod app;
#[allow(dead_code)]
mod theme;

pub use app::HumApp;

use eframe::egui;
use winit::application::ApplicationHandler;
use winit::event_loop::{ActiveEventLoop, EventLoop};

/// Wraps eframe's winit application so we can see lifecycle events
/// (suspend / resume) that eframe does not forward to the app.
struct Lifecycle<'a> {
    inner: eframe::EframeWinitApplication<'a>,
}

impl ApplicationHandler<eframe::UserEvent> for Lifecycle<'_> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        log::info!("lifecycle: resumed");
        self.inner.resumed(event_loop);
    }

    fn suspended(&mut self, event_loop: &ActiveEventLoop) {
        log::info!("lifecycle: suspended");
        self.inner.suspended(event_loop);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: winit::window::WindowId,
        event: winit::event::WindowEvent,
    ) {
        self.inner.window_event(event_loop, window_id, event);
    }

    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: winit::event::StartCause) {
        self.inner.new_events(event_loop, cause);
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: eframe::UserEvent) {
        self.inner.user_event(event_loop, event);
    }

    fn device_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        device_id: winit::event::DeviceId,
        event: winit::event::DeviceEvent,
    ) {
        self.inner.device_event(event_loop, device_id, event);
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.inner.about_to_wait(event_loop);
    }

    fn exiting(&mut self, event_loop: &ActiveEventLoop) {
        self.inner.exiting(event_loop);
    }

    fn memory_warning(&mut self, event_loop: &ActiveEventLoop) {
        self.inner.memory_warning(event_loop);
    }
}

fn run(event_loop: EventLoop<eframe::UserEvent>, options: eframe::NativeOptions) {
    let inner = eframe::create_native(
        "Hum",
        options,
        Box::new(|cc| Ok(Box::new(HumApp::new(cc)))),
        &event_loop,
    );
    let mut app = Lifecycle { inner };
    if let Err(err) = event_loop.run_app(&mut app) {
        log::error!("event loop ended with an error: {err}");
    }
}

/// Desktop dev mode: a 390x844 window, the mouse acts as a finger.
#[cfg(not(target_os = "android"))]
pub fn desktop_main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Hum")
            .with_inner_size([390.0, 844.0])
            .with_min_inner_size([320.0, 560.0]),
        ..Default::default()
    };
    let event_loop = EventLoop::<eframe::UserEvent>::with_user_event()
        .build()
        .expect("failed to create the event loop");
    run(event_loop, options);
}

/// Android entry point, called by android-activity's NativeActivity glue.
#[cfg(target_os = "android")]
#[no_mangle]
fn android_main(app: winit::platform::android::activity::AndroidApp) {
    use winit::platform::android::EventLoopBuilderExtAndroid as _;

    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Info)
            .with_tag("hum"),
    );
    let options = eframe::NativeOptions {
        android_app: Some(app.clone()),
        ..Default::default()
    };
    let event_loop = EventLoop::<eframe::UserEvent>::with_user_event()
        .with_android_app(app)
        .build()
        .expect("failed to create the event loop");
    run(event_loop, options);
}
