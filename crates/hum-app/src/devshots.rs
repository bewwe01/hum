//! Desktop-only developer aid: `HUM_SHOTS=<dir> cargo run -p hum-app` walks
//! through the screens, saves a screenshot of each as a .ppm file and exits.
//! Used to check the layout against the design without a phone.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use eframe::egui;
use hum_engine::engine::Cmd;

use crate::app::{HumApp, Sheet, Tab};

pub struct Shots {
    dir: PathBuf,
    step: usize,
    wait: u32,
    requested: bool,
    slot: Arc<Mutex<Option<Arc<egui::ColorImage>>>>,
}

const NAMES: [&str; 12] = [
    "play",
    "play-keys",
    "sounds",
    "loop",
    "loop-layers",
    "play-looped",
    "beats",
    "settings",
    "play-drums",
    "sheet-key",
    "songs",
    "song",
];

impl Shots {
    pub fn from_env() -> Option<Self> {
        let dir = PathBuf::from(std::env::var_os("HUM_SHOTS")?);
        let _ = std::fs::create_dir_all(&dir);
        Some(Self {
            dir,
            step: 0,
            wait: 0,
            requested: false,
            slot: Arc::default(),
        })
    }
}

fn arrange(app: &mut HumApp, step: usize) {
    app.settings_open = false;
    app.sheet = None;
    match step {
        0 => app.tab = Tab::Play,
        1 => {
            app.tab = Tab::Play;
            app.st.play_mode = crate::keyboard::PlayMode::Piano;
            app.zone_down[2] = 1;
            app.zone_down[9] = 1;
            app.st.pad_x = 0.68;
            app.st.pad_y = 0.62;
            let now = app.now;
            app.trail = (0..14)
                .map(|i| {
                    (
                        0.3 + 0.027 * i as f32,
                        0.3 + 0.023 * i as f32,
                        now + 100.0 - 0.04 * (14 - i) as f64,
                    )
                })
                .collect();
        }
        2 => {
            app.zone_down = [0; crate::keyboard::MAX_ZONES];
            app.trail.clear();
            app.tab = Tab::Sounds;
        }
        3 => app.tab = Tab::Loop,
        4 => {
            // A demo loop: 2 bars, three layers with different sounds.
            use crate::state::{LoopLayer, LoopNote, Looper};
            let mut looper = Looper::new(32);
            let layer = |sound: &str, notes: &[(u8, f32, f32)], muted: bool| LoopLayer {
                sound: sound.to_owned(),
                muted,
                notes: notes
                    .iter()
                    .map(|&(note, on, off)| LoopNote {
                        note,
                        velocity: 100,
                        on,
                        off,
                    })
                    .collect(),
                ..LoopLayer::default()
            };
            looper.layers[0] = Some(layer(
                "Warm Cloud",
                &[(48, 0.0, 15.0), (53, 16.0, 31.0)],
                false,
            ));
            looper.layers[1] = Some(layer(
                "Pluck Harp",
                &[
                    (60, 2.0, 3.0),
                    (63, 6.0, 7.0),
                    (67, 10.0, 11.0),
                    (65, 18.0, 19.0),
                    (63, 22.0, 23.0),
                ],
                false,
            ));
            looper.layers[2] = Some(layer(
                "Sub Thump",
                &[(36, 0.0, 2.0), (36, 8.0, 10.0), (41, 16.0, 18.0)],
                true,
            ));
            looper.order = vec![0, 1, 2];
            app.st.looper = Some(looper);
            app.send(Cmd::SetBpm(110.0));
            app.send(Cmd::SetLoopSteps(32));
            for slot in 0..3 {
                app.send_layer(slot);
            }
            app.send(Cmd::LoopPlay(true));
            app.tab = Tab::Loop;
            if std::env::var_os("HUM_SHOTS_EXPORT").is_some() {
                app.export_loop();
            }
        }
        5 => {
            app.st.play_mode = crate::keyboard::PlayMode::Chords;
            app.tab = Tab::Play;
        }
        6 => {
            app.send(Cmd::BeatPlay(true));
            app.tab = Tab::Beats;
        }
        7 => {
            app.send(Cmd::BeatPlay(false));
            app.tab = Tab::Play;
            app.settings_open = true;
        }
        8 => {
            app.st.play_mode = crate::keyboard::PlayMode::Drums;
            app.tab = Tab::Loop;
            app.sheet = Some(Sheet::Layer(1));
        }
        9 => {
            app.tab = Tab::Play;
            app.sheet = Some(Sheet::Key);
        }
        10 => {
            app.st.play_mode = crate::keyboard::PlayMode::Piano;
            app.st.songs = vec![crate::songs::Song::from_text(DEMO_SONG)];
            app.tab = Tab::Songs;
        }
        _ => {
            app.tab = Tab::Songs;
            app.open_song_for_shot();
        }
    }
}

/// A made-up chord sheet in the format tab sites use.
const DEMO_SONG: &str = "Morning Walk\nCapo 2\n\n[Intro]\nG  D  Em  C\n\n[Verse 1]\n\
G                D\nWoke up early, sun is on the hill\nEm               C\nCoffee on the window sill\n\
G                D/F#\nEvery street is quiet still\nEm         C        D\nWalking slow, I always will\n\n\
[Chorus]\nC        G         D       Em\nOh the morning, oh the light\nC        G         D\nEverything is \
going right\n";

pub fn drive(app: &mut HumApp, ctx: &egui::Context) {
    let Some(mut shots) = app.shots.take() else {
        return;
    };
    ctx.request_repaint();
    // Save a screenshot that arrived this frame.
    let image = shots.slot.lock().ok().and_then(|mut s| s.take());
    if let Some(image) = image {
        let path = shots.dir.join(format!("{}.ppm", NAMES[shots.step]));
        let mut data = format!("P6\n{} {}\n255\n", image.width(), image.height()).into_bytes();
        for px in image.pixels.iter() {
            data.extend_from_slice(&[px.r(), px.g(), px.b()]);
        }
        if let Err(err) = std::fs::write(&path, data) {
            log::error!("could not write {}: {err}", path.display());
        }
        shots.step += 1;
        shots.wait = 0;
        shots.requested = false;
        if shots.step >= NAMES.len() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
    }
    if shots.wait == 0 {
        arrange(app, shots.step);
    }
    shots.wait += 1;
    // Let the screen settle (and the fake recording run) before the shot.
    let frames = if shots.step == 4 { 40 } else { 12 };
    if shots.wait == frames && !shots.requested {
        let slot = shots.slot.clone();
        let repaint = ctx.clone();
        ctx.request_screenshot(move |image| {
            if let Ok(mut s) = slot.lock() {
                *s = Some(image);
            }
            repaint.request_repaint();
        });
        shots.requested = true;
    }
    app.shots = Some(shots);
}
