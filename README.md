# Hum

A synth and instrument playground for complete beginners. Open it, pick a
sound, play. Notes can be locked to a scale, so nothing sounds wrong.

- **197 sounds:** pianos, keys, guitars, basses, strings, brass and wind,
  choirs, world instruments and 13 drum kits from the bundled GeneralUser
  GS SoundFont, plus Hum's own 57 synth sounds (including an Ocarina).
- **Capture looping:** play first, then tap **Loop** and what you just
  played repeats. The first loop sets the length and the tempo. Each
  later tap adds what you played over the loop as a new layer, up to 8.
  Every layer has its own on/off switch, volume, pan, reverb, "tidy
  timing", and a sound you can change after recording. There is undo
  and redo, the loop is saved, and you can export it as a WAV file to
  Downloads/Hum.
- **Four ways to play:** scale-locked keys, piano keys, chord pads and
  drum pads. Where you touch a key sets how loud it plays. Turn the phone
  sideways for a wider piano (two octaves or more) and 14 scale keys.
- **Legato** (Settings): one note at a time with glides between notes.
  *Auto* turns it on for wind instruments.
- **Songs:** copy a chord sheet (chords above lyrics, as on tab sites) in
  your browser and tap **Paste**, or type one. Hum shows the sheet with the
  current chord highlighted, a strum pad (tap, or swipe down and up), a
  pad per chord, **Next** to follow the song, key and capo, guitar or
  piano voicings, and **Auto**, which strums or plays a pattern on the
  beat. What you play there can be captured as a loop layer. Nothing is
  downloaded: Hum has no network access.
- A 16-step beat maker on the same clock.

Hum is a personal Android app for a Pixel (arm64) running GrapheneOS. It
**requests zero permissions**, has no network code and no analytics, and
needs no Google Play Services. The looper records the notes you play, never
microphone audio.

Everything is Rust: the UI, the audio and the synth. The only other files are
the small Gradle project and manifest needed to package the APK.

```
crates/
  hum-engine/   pure DSP: oscillators, filters, envelopes, effects, limiter,
                presets (the 57-sound table), scales, chords and voicings,
                arpeggiator, beat sequencer, looper, strums and accompaniment. No platform code, fully host-testable.
  hum-app/      egui UI + cpal audio glue. cdylib for Android, binary for desktop.
  hum-tools/    `render`: renders sounds to .wav and prints measurements.
android/        minimal NativeActivity Gradle project (no Java/Kotlin code)
docs/RELEASING.md   signing setup, tagging, installing
```

## Build and run

The toolchain is pinned in `rust-toolchain.toml` (Rust 1.97.0). `rustup`
picks it up automatically.

### Desktop (for UI work)

```sh
# Linux needs the ALSA headers once: sudo apt install libasound2-dev pkg-config
cargo run -p hum-app
```

This opens a 390x844 window (the design's reference size) where the mouse
acts as one finger. Settings are stored in `~/.config/hum/hum-state.json`; set
`HUM_DATA_DIR` to use another directory. With no audio device, the engine runs
silently so the beat and loop clocks still move. Escape works like the Android
back button.

To screenshot every screen without clicking around:

```sh
HUM_SHOTS=shots/ cargo run -p hum-app     # writes shots/*.ppm and exits
HUM_LANDSCAPE=1 cargo run -p hum-app      # an 844x390 (landscape) window
```

The desktop build has no clipboard; `HUM_PASTE_FILE=sheet.txt` makes the
Songs screen's Paste button read that file.

### Android APK

CI builds a debug APK on every run (see below). To build one locally you need
the Android SDK with NDK `27.3.13750724`, `platforms;android-36` and
`build-tools;36.0.0`, plus JDK 17 or 21:

```sh
rustup target add aarch64-linux-android        # already in rust-toolchain.toml
cargo install cargo-ndk --locked --version 4.1.2
export ANDROID_HOME=~/Android/Sdk ANDROID_NDK_HOME=$ANDROID_HOME/ndk/27.3.13750724

cargo ndk --target arm64-v8a --platform 26 \
  --output-dir android/app/src/main/jniLibs \
  build --package hum-app --lib --release --locked
(cd android && ./gradlew assembleDebug)
adb install -r android/app/build/outputs/apk/debug/app-debug.apk
```

`scripts/check-apk.sh <apk>` prints the APK's permissions (there must be
none), SDK levels and native libraries.

The application id `com.example.hum` is defined in exactly one place:
`humApplicationId` in [`android/gradle.properties`](android/gradle.properties).
Change it there before your first real release, because changing it later
installs as a different app.

## Test

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
# Type-check the Android JNI glue on the host:
cargo clippy -p hum-app --features jni-check -- -D warnings
```

The engine tests (`crates/hum-engine/tests/engine.rs` plus the unit tests in
each module) check:

- every one of the 57 sounds renders finite, non-silent audio,
- the output never exceeds 0 dBFS, even with every key held on every sound
  with all controls at their extremes (the limiter ceiling is 0.95),
- note-off always decays to silence and frees its voices (25-second check),
- voice stealing and "all notes off" never jump more than 0.05 between
  samples (a click detector on a pure low sine),
- scales map degrees to the right semitones for every root, scale and octave,
- the sequencer starts each step on the exact sample at 60, 97, 110, 120,
  133 and 160 BPM,
- the looper replays recorded notes on the recorded sample, mutes, clears,
  discards empty takes, and keeps each layer's own sound,
- the arpeggiator walks the scale in time with the shared clock,
- legato keeps one voice, strums spread their notes in time, and the
  accompaniment changes chord on the right step,
- chord symbols parse and voice correctly; chord sheets are read from
  chords-over-lyrics and ChordPro text.

## Listen to the sounds

I can't hear, so the sounds have been measured but never listened to. The
render tool exists so you can listen:

```sh
cargo run --release -p hum-tools --bin render -- --sound 'Warm Cloud' --note C4 --out out.wav
cargo run --release -p hum-tools --bin render -- --all --outdir renders/
cargo run --release -p hum-tools --bin render -- --list
```

`--all` writes `renders/01-laser-lead.wav` … `57-ocarina.wav` and prints
peak, RMS, spectral centroid ("brightness") and tail length for each sound.
`--x` and `--y` set the XY pad position and `--hold` sets the note length.

Preset gains were levelled from those measurements: sustained sounds sit
near -19 dBFS RMS and percussive sounds peak near -7 dBFS. Whether they
*sound* good, and whether each one matches its description, still needs your
ears.

## Release

**Actions → Release → Run workflow**, then type a version or leave it empty
to bump the patch number. The workflow builds, zipaligns, signs and verifies
the APK, checks it has no permissions, creates the tag and publishes
`hum-<tag>-arm64-v8a.apk` with a `.sha256`. Without signing secrets it signs
with a temporary key, so you uninstall before each update. With your own
keystore (see **[docs/RELEASING.md](docs/RELEASING.md)**) updates install in
place. Pushing a `vX.Y.Z` tag yourself works too.

## CI

- `.github/workflows/ci.yml`: on pushes to `main`, pull requests and manual
  runs. Runs fmt, clippy, the tests and a debug arm64 APK build, which is
  uploaded as the `hum-debug-arm64-v8a` artifact.
- `.github/workflows/release.yml`: started from the Actions tab (it
  creates the tag) or by pushing a `v*.*.*` tag.
- Third-party actions are pinned to commit SHAs, the default permission is
  `contents: read`, and Dependabot watches the actions and the crates.

## How it works

**Audio thread.** The cpal output callback owns the `Engine`. The UI sends
notes and other discrete changes through a lock-free SPSC queue (`rtrb`).
Knobs and the XY pad are atomics, and status comes back through atomics too.
The callback never allocates, locks or does I/O. All buffers are allocated
up front, and denormals are flushed (the FZ bit on arm64, plus explicit
flushing in feedback paths). When the stream closes (app in the background,
headphones unplugged, buffer setting changed) the engine is handed back to
the UI thread, so loops and beats survive.

**Engine.** 24 voices with click-free stealing (4 ms fade). Each voice has up
to three oscillator layers (PolyBLEP saw and pulse, triangle, sine, noise),
up to 7 unison voices, a sub, 2-op FM, ring mod, a state-variable filter
(LP/BP/HP, 24 dB cascade, formant mode) with key tracking and an envelope,
amp, filter and pitch envelopes, one LFO (tempo-sync option), glide, drive and
a bit-crusher. Shared bus: chorus, tempo-aware ping-pong delay,
Freeverb-style reverb (the REVERB knob is the send), then a soft limiter with
an instant-attack peak follower, which bounds the output below 0 dBFS by
construction. Control changes are smoothed every 16 samples.

**One clock.** Beats, the looper and the arpeggiator share one transport,
counted in sixteenth-note steps from an exact sample position. The engine
splits each audio block at event boundaries, so events land on exact samples.

**Presets are data.** `crates/hum-engine/src/presets.rs` is one table of 57
`Preset` structs. The engine has no per-sound code paths.

**UI.** egui (eframe, OpenGL ES via glow) on `android-activity`'s
NativeActivity. All widgets are custom-painted from the design tokens in
`crates/hum-app/src/theme.rs`. A small multi-touch layer
(`crates/hum-app/src/input.rs`) lets several fingers play chords while
another moves the pad. The app repaints only while something moves or is
being touched.

## Decisions and deviations

- **cpal talks to AAudio directly, not Oboe.** cpal 0.18 replaced its Oboe
  backend with direct AAudio, which is what Oboe uses on Android 8.1+ anyway.
  With cpal's `realtime` feature the stream asks for the low-latency
  performance mode. Hum requests 48 kHz. *Auto* lets AAudio size and tune the
  buffer, *Low* asks for 96-frame callbacks and *Safe* for 960. Settings shows
  the latency measured from the stream's timestamps.
- **glow instead of wgpu.** It gives a smaller APK and fewer moving parts.
  egui's OpenGL ES backend is mature on Android.
- **minSdk 26** (Android 8.0) is the floor for AAudio. **targetSdk 36** is
  the current Google Play requirement (from 31 August 2026).
- **Haptics use `View.performHapticFeedback(KEYBOARD_TAP)`,** which needs no
  permission. The Vibrator service would need `VIBRATE`, which Hum does not
  request. The buzz follows the system's "Touch feedback" setting: if that is
  off in Android settings, Hum's switch has no effect.
- **Audio focus.** Hum requests focus while its stream is open, so other
  music pauses, and abandons it in the background. It cannot react to
  *losing* focus (for example a phone call), because that needs a Java
  listener class and the app has no Java code. Hum closes its stream whenever
  it leaves the foreground, which covers the common case.
- **Back button.** It closes an open sheet, then leaves Settings, then
  returns to Play. On Play it moves the app to the background.
- **Fonts.** Space Grotesk (variable, weights picked per style) and Inter
  400/500/600 are bundled from their upstream repositories with their OFL
  licence texts (`assets/fonts/`). Neither font has a ♭ glyph, so the flat
  sign is drawn as a vector.
- **Key and scale pickers** are bottom sheets. The design shows the pills
  but not what happens when you tap them.
- **Landscape** keeps the keyboard big: the XY pad and the four knobs are
  only on the upright Play screen.
- **Narrow phones.** The keyboard fills the leftover height. On short
  screens the XY pad shrinks (minimum 120) before the keyboard drops below
  its minimum of 140.

## Licence of bundled assets

GeneralUser GS v2.0.3 by S. Christian Collins
(`assets/soundfonts/GeneralUser-GS-LICENSE.txt`): free to use in music and
in software projects.

Space Grotesk: SIL Open Font License 1.1 (`assets/fonts/SpaceGrotesk-OFL.txt`).
Inter: SIL Open Font License 1.1 (`assets/fonts/Inter-LICENSE.txt`).
