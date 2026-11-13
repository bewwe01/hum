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

    pub fn set_keep_screen_on(&self, _on: bool) {}

    pub fn insets(&self) -> Insets {
        Insets::default()
    }
}

#[cfg(target_os = "android")]
impl Platform {
    pub fn android(app: winit::platform::android::activity::AndroidApp) -> Self {
        Self { app }
    }

    /// The app's private files directory (no permission needed).
    pub fn data_dir(&self) -> Option<PathBuf> {
        self.app.internal_data_path()
    }

    /// A short key-press buzz through `View.performHapticFeedback`, which
    /// (unlike the Vibrator service) needs no VIBRATE permission. It follows
    /// the system's "touch feedback" setting.
    pub fn haptic_tick(&self) {
        jni_glue::haptic_tick();
    }

    pub fn set_keep_screen_on(&self, on: bool) {
        use winit::platform::android::activity::WindowManagerFlags as Flags;
        if on {
            self.app
                .set_window_flags(Flags::KEEP_SCREEN_ON, Flags::empty());
        } else {
            self.app
                .set_window_flags(Flags::empty(), Flags::KEEP_SCREEN_ON);
        }
    }

    pub fn insets(&self) -> Insets {
        let sdk = winit::platform::android::activity::AndroidApp::sdk_version();
        jni_glue::insets(sdk).unwrap_or_default()
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

    /// Run `f` with a JNI environment and the Activity. Any Java exception
    /// is cleared so that a failed call can never crash the app.
    fn with_activity<R>(
        f: impl for<'j> FnOnce(&mut Env<'j>, &JObject<'j>) -> JResult<R>,
    ) -> Option<R> {
        let ctx = ndk_context::android_context();
        if ctx.vm().is_null() || ctx.context().is_null() {
            return None;
        }
        // SAFETY: android-activity initialised ndk-context with the process's
        // JavaVM and a global reference to the Activity; both stay valid for
        // the life of the process.
        let vm = unsafe { JavaVM::from_raw(ctx.vm().cast()) };
        let raw_activity = ctx.context() as jni::sys::jobject;
        vm.attach_current_thread(|env: &mut Env<'_>| {
            // SAFETY: see above; the reference is a valid global reference.
            let activity = unsafe { JObject::from_raw(env, raw_activity) };
            let result = f(env, &activity);
            if result.is_err() {
                env.exception_clear();
            }
            result
        })
        .ok()
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
