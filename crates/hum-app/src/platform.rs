//! The few things that need the operating system: where to store settings,
//! haptics, keeping the screen on and the system-bar insets.
//!
//! On Android these go through `android-activity` and a handful of JNI
//! calls. None of them needs a permission.

use std::path::PathBuf;

/// Insets of the system bars / display cutout / on-screen keyboard, in
/// physical pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Insets {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

#[derive(Clone)]
pub struct Platform {
    #[cfg(target_os = "android")]
    app: winit::platform::android::activity::AndroidApp,
    #[cfg(target_os = "android")]
    worker: android_worker::Worker,
}

#[cfg(not(target_os = "android"))]
impl Platform {
    pub fn desktop() -> Self {
        Self {}
    }

    /// Directory for the settings file.
    pub fn data_dir(&self) -> Option<PathBuf> {
        if let Some(dir) = std::env::var_os("HUM_DATA_DIR") {
            return Some(PathBuf::from(dir));
        }
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .or_else(|| std::env::var_os("APPDATA").map(PathBuf::from))?;
        Some(base.join("hum"))
    }

    pub fn haptic_tick(&self) {}

    /// Parse the bundled SoundFont (from the repository's assets folder).
    pub fn load_soundfont(&self) -> crate::audio::LoadedSoundFont {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/soundfonts/GeneralUser-GS.sf2"
        );
        let file = std::fs::File::open(path).map_err(|e| format!("{path}: {e}"))?;
        let mut reader = std::io::BufReader::new(file);
        rustysynth::SoundFont::new(&mut reader)
            .map(std::sync::Arc::new)
            .map_err(|e| format!("{e:?}"))
    }

    pub fn set_keep_screen_on(&self, _on: bool) {}

    pub fn set_audio_focus(&self, _focused: bool) {}

    /// The system back action on the first screen. Nothing to do on desktop.
    pub fn leave(&self) {}

    pub fn insets(&self) -> Insets {
        Insets::default()
    }
}

/// Every call into the Android framework is posted to the Java main thread
/// (see `android_worker`), so it can never block or deadlock the UI. The UI
/// only queues requests and reads cached results.
#[cfg(target_os = "android")]
impl Platform {
    pub fn android(app: winit::platform::android::activity::AndroidApp) -> Self {
        let worker = android_worker::Worker::start(app.clone());
        Self { app, worker }
    }

    /// The app's private files directory (no permission needed).
    pub fn data_dir(&self) -> Option<PathBuf> {
        self.app.internal_data_path()
    }

    /// Parse the SoundFont packaged in the APK's assets.
    pub fn load_soundfont(&self) -> crate::audio::LoadedSoundFont {
        let asset = self
            .app
            .asset_manager()
            .open(c"GeneralUser-GS.sf2")
            .ok_or("GeneralUser-GS.sf2 is missing from the APK assets")?;
        let mut reader = std::io::BufReader::with_capacity(1 << 20, asset);
        rustysynth::SoundFont::new(&mut reader)
            .map(std::sync::Arc::new)
            .map_err(|e| format!("{e:?}"))
    }

    /// A short key-press buzz through `View.performHapticFeedback`, which
    /// (unlike the Vibrator service) needs no VIBRATE permission. It follows
    /// the system's "touch feedback" setting.
    pub fn haptic_tick(&self) {
        self.worker.send(android_worker::Job::Haptic);
    }

    pub fn set_keep_screen_on(&self, on: bool) {
        self.worker.send(android_worker::Job::KeepScreenOn(on));
    }

    /// Ask other media apps to pause while Hum makes sound (and let them
    /// resume when it stops). Needs no permission.
    pub fn set_audio_focus(&self, focused: bool) {
        self.worker.send(android_worker::Job::AudioFocus(focused));
    }

    /// Back on the first screen: send the app to the background, as the
    /// system would for a normal Activity.
    pub fn leave(&self) {
        self.worker.send(android_worker::Job::Leave);
    }

    /// The last measured insets; also queues a fresh measurement.
    pub fn insets(&self) -> Insets {
        self.worker.send(android_worker::Job::RefreshInsets);
        self.worker.insets()
    }
}

/// Runs platform jobs on Android's Java main (UI) thread.
///
/// Why not call android-activity directly from the UI code: while the app's
/// event-loop callbacks run, android-activity holds a read lock on its
/// state, and `AndroidApp::set_window_flags` takes the write lock on that
/// same lock. Calling it from inside the event loop deadlocks forever (the
/// black screen and ANR in v0.1.0); calling it from another thread
/// deadlocks the next frame instead. Posting work to the Java main thread
/// only needs the read lock, and window/view calls belong on that thread
/// anyway.
#[cfg(target_os = "android")]
mod android_worker {
    use super::{jni_glue, Insets};
    use std::sync::{Arc, Mutex};
    use std::time::Instant;
    use winit::platform::android::activity::AndroidApp;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Job {
        Haptic,
        KeepScreenOn(bool),
        AudioFocus(bool),
        Leave,
        RefreshInsets,
    }

    #[derive(Clone)]
    pub struct Worker {
        app: AndroidApp,
        insets: Arc<Mutex<Insets>>,
        seen: Arc<Mutex<Vec<Job>>>,
    }

    impl Worker {
        pub fn start(app: AndroidApp) -> Self {
            jni_glue::set_target(app.vm_as_ptr(), app.activity_as_ptr());
            Self {
                app,
                insets: Arc::default(),
                seen: Arc::default(),
            }
        }

        /// Post a job to the Java main thread; never blocks the caller.
        pub fn send(&self, job: Job) {
            let insets = self.insets.clone();
            let seen = self.seen.clone();
            self.app.run_on_java_main_thread(Box::new(move || {
                let first = seen.lock().map(|mut s| {
                    let first = !s.contains(&job);
                    if first {
                        s.push(job);
                    }
                    first
                });
                let first = first.unwrap_or(false);
                let started = Instant::now();
                match job {
                    Job::Haptic => jni_glue::haptic_tick(),
                    Job::KeepScreenOn(on) => jni_glue::keep_screen_on(on),
                    Job::AudioFocus(focused) => jni_glue::audio_focus(focused),
                    Job::Leave => jni_glue::move_task_to_back(),
                    Job::RefreshInsets => {
                        let sdk = AndroidApp::sdk_version();
                        if let Some(value) = jni_glue::insets(sdk) {
                            if let Ok(mut slot) = insets.lock() {
                                *slot = value;
                            }
                        }
                    }
                }
                if first {
                    log::info!(
                        "platform: first {job:?} took {} ms",
                        started.elapsed().as_millis()
                    );
                }
            }));
        }

        pub fn insets(&self) -> Insets {
            self.insets.lock().map(|i| *i).unwrap_or_default()
        }
    }
}

/// JNI calls into the Android framework. Compiled for Android, and for the
/// host with `--features jni-check` so it can be type-checked without an NDK.
#[cfg(any(target_os = "android", feature = "jni-check"))]
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
mod jni_glue {
    use super::Insets;
    use jni::errors::Result as JResult;
    use jni::objects::{JObject, JValue};
    use jni::{jni_sig, jni_str, Env, JavaVM};

    /// `HapticFeedbackConstants.KEYBOARD_TAP`.
    const KEYBOARD_TAP: i32 = 3;
    /// `WindowInsets.Type`: systemBars() | ime() | displayCutout().
    const INSET_TYPES: i32 = 0x7 | 0x8 | 0x80;

    /// The JavaVM and the Activity (raw pointers), registered by the
    /// platform thread from `AndroidApp`. (`ndk-context` holds the
    /// Application, which has no window, so it cannot be used here.)
    static TARGET: std::sync::OnceLock<(usize, usize)> = std::sync::OnceLock::new();

    pub fn set_target(vm: *mut std::ffi::c_void, activity: *mut std::ffi::c_void) {
        let _ = TARGET.set((vm as usize, activity as usize));
    }

    /// Run `f` with a JNI environment and the Activity. Any Java exception
    /// is cleared so that a failed call can never crash the app.
    fn with_activity<R>(
        f: impl for<'j> FnOnce(&mut Env<'j>, &JObject<'j>) -> JResult<R>,
    ) -> Option<R> {
        let &(vm, activity) = TARGET.get()?;
        if vm == 0 || activity == 0 {
            return None;
        }
        // SAFETY: both pointers come from the AndroidApp that the platform
        // thread keeps alive: the process's JavaVM and a global reference to
        // the Activity.
        let vm = unsafe { JavaVM::from_raw(vm as *mut jni::sys::JavaVM) };
        let raw_activity = activity as jni::sys::jobject;
        let result = vm.attach_current_thread(|env: &mut Env<'_>| {
            // SAFETY: see above; the reference is a valid global reference.
            let activity = unsafe { JObject::from_raw(env, raw_activity) };
            let result = f(env, &activity);
            if result.is_err() {
                env.exception_clear();
            }
            result
        });
        if let Err(err) = &result {
            log::warn!("platform: JNI call failed: {err}");
        }
        result.ok()
    }

    fn decor_view<'j>(env: &mut Env<'j>, activity: &JObject<'j>) -> JResult<JObject<'j>> {
        let window = env
            .call_method(
                activity,
                jni_str!("getWindow"),
                jni_sig!("()Landroid/view/Window;"),
                &[],
            )?
            .l()?;
        env.call_method(
            &window,
            jni_str!("getDecorView"),
            jni_sig!("()Landroid/view/View;"),
            &[],
        )?
        .l()
    }

    pub fn haptic_tick() {
        with_activity(|env, activity| {
            let view = decor_view(env, activity)?;
            env.call_method(
                &view,
                jni_str!("performHapticFeedback"),
                jni_sig!("(I)Z"),
                &[JValue::Int(KEYBOARD_TAP)],
            )?
            .z()
        });
    }

    /// `AudioManager.STREAM_MUSIC` and `AudioManager.AUDIOFOCUS_GAIN`.
    const STREAM_MUSIC: i32 = 3;
    const AUDIOFOCUS_GAIN: i32 = 1;

    /// Request or abandon audio focus. This uses the long-standing
    /// listener-less form: Hum cannot implement a Java listener without Java
    /// code, and it already goes quiet whenever it leaves the foreground.
    pub fn audio_focus(focused: bool) {
        with_activity(|env, activity| {
            let name = env.new_string("audio")?;
            let manager = env
                .call_method(
                    activity,
                    jni_str!("getSystemService"),
                    jni_sig!("(Ljava/lang/String;)Ljava/lang/Object;"),
                    &[JValue::Object(&name)],
                )?
                .l()?;
            let none = JObject::null();
            if focused {
                env.call_method(
                    &manager,
                    jni_str!("requestAudioFocus"),
                    jni_sig!("(Landroid/media/AudioManager$OnAudioFocusChangeListener;II)I"),
                    &[
                        JValue::Object(&none),
                        JValue::Int(STREAM_MUSIC),
                        JValue::Int(AUDIOFOCUS_GAIN),
                    ],
                )?
                .i()
            } else {
                env.call_method(
                    &manager,
                    jni_str!("abandonAudioFocus"),
                    jni_sig!("(Landroid/media/AudioManager$OnAudioFocusChangeListener;)I"),
                    &[JValue::Object(&none)],
                )?
                .i()
            }
        });
    }

    /// `WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON`.
    const FLAG_KEEP_SCREEN_ON: i32 = 0x80;

    /// Add or clear FLAG_KEEP_SCREEN_ON on the Activity's window. Must run on
    /// the Java main thread.
    pub fn keep_screen_on(on: bool) {
        with_activity(|env, activity| {
            let window = env
                .call_method(
                    activity,
                    jni_str!("getWindow"),
                    jni_sig!("()Landroid/view/Window;"),
                    &[],
                )?
                .l()?;
            let method = if on {
                jni_str!("addFlags")
            } else {
                jni_str!("clearFlags")
            };
            env.call_method(
                &window,
                method,
                jni_sig!("(I)V"),
                &[JValue::Int(FLAG_KEEP_SCREEN_ON)],
            )?;
            Ok(())
        });
    }

    pub fn move_task_to_back() {
        with_activity(|env, activity| {
            env.call_method(
                activity,
                jni_str!("moveTaskToBack"),
                jni_sig!("(Z)Z"),
                &[JValue::Bool(true)],
            )?
            .z()
        });
    }

    pub fn insets(sdk: i32) -> Option<Insets> {
        with_activity(|env, activity| {
            let view = decor_view(env, activity)?;
            let window_insets = env
                .call_method(
                    &view,
                    jni_str!("getRootWindowInsets"),
                    jni_sig!("()Landroid/view/WindowInsets;"),
                    &[],
                )?
                .l()?;
            if window_insets.is_null() {
                return Ok(Insets::default());
            }
            if sdk >= 30 {
                let insets = env
                    .call_method(
                        &window_insets,
                        jni_str!("getInsets"),
                        jni_sig!("(I)Landroid/graphics/Insets;"),
                        &[JValue::Int(INSET_TYPES)],
                    )?
                    .l()?;
                let mut field = |name| -> JResult<f32> {
                    Ok(env.get_field(&insets, name, jni_sig!("I"))?.i()? as f32)
                };
                Ok(Insets {
                    left: field(jni_str!("left"))?,
                    top: field(jni_str!("top"))?,
                    right: field(jni_str!("right"))?,
                    bottom: field(jni_str!("bottom"))?,
                })
            } else {
                let mut call = |name| -> JResult<f32> {
                    Ok(env
                        .call_method(&window_insets, name, jni_sig!("()I"), &[])?
                        .i()? as f32)
                };
                Ok(Insets {
                    left: call(jni_str!("getSystemWindowInsetLeft"))?,
                    top: call(jni_str!("getSystemWindowInsetTop"))?,
                    right: call(jni_str!("getSystemWindowInsetRight"))?,
                    bottom: call(jni_str!("getSystemWindowInsetBottom"))?,
                })
            }
        })
    }
}
