//! Render Hum's sounds to wav files and print measurements.
//!
//! ```text
//! cargo run -p hum-tools --bin render -- --sound 'Warm Cloud' --note C4 --out out.wav
//! cargo run -p hum-tools --bin render -- --all --outdir renders/
//! cargo run -p hum-tools --bin render -- --list
//! ```

use std::path::PathBuf;
use std::process::ExitCode;

use hum_engine::scale::{note_name_ascii, parse_note};
use hum_engine::{presets, PRESETS};
use hum_tools::analysis::db;
use hum_tools::render::{file_stem, render, RenderSpec, Rendered, SAMPLE_RATE};
use hum_tools::wav;

const USAGE: &str = "\
Render Hum sounds to 48 kHz stereo wav files.

USAGE:
  render --sound <NAME> [--note C4] [--out out.wav] [options]
  render --all [--outdir renders/] [options]
  render --list

OPTIONS:
  --sound <NAME>   Sound name, e.g. 'Warm Cloud' (case-insensitive)
  --note <NOTE>    Note name or MIDI number (default: C4, C3 for Bass)
  --out <FILE>     Output file for --sound (default: out.wav)
  --all            Render all 56 sounds
  --outdir <DIR>   Output directory for --all (default: renders/)
  --hold <SECS>    How long the key is held
  --x <0..1>       XY pad brightness (default 0.5)
  --y <0..1>       XY pad wobble (default 0.3)
  --gm <BANK:PROG> Render a SoundFont instrument instead, e.g. 0:0 (piano)
  --gm-survey      Render a spread of SoundFont instruments and print levels
  --list           List the sounds and exit";

#[derive(Default)]
struct Args {
    sound: Option<String>,
    note: Option<u8>,
    out: Option<PathBuf>,
    all: bool,
    outdir: Option<PathBuf>,
    hold: Option<f32>,
    x: Option<f32>,
    y: Option<f32>,
    list: bool,
    gm: Option<(u16, u16)>,
    gm_survey: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args::default();
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let mut value = |name: &str| it.next().ok_or(format!("{name} needs a value"));
        match flag.as_str() {
            "--sound" => args.sound = Some(value("--sound")?),
            "--note" => {
                let v = value("--note")?;
                args.note = Some(parse_note(&v).ok_or(format!("cannot parse note '{v}'"))?);
            }
            "--out" => args.out = Some(PathBuf::from(value("--out")?)),
            "--all" => args.all = true,
            "--outdir" => args.outdir = Some(PathBuf::from(value("--outdir")?)),
            "--hold" => {
                args.hold = Some(
                    value("--hold")?
                        .parse()
                        .map_err(|e| format!("--hold: {e}"))?,
                )
            }
            "--x" => args.x = Some(value("--x")?.parse().map_err(|e| format!("--x: {e}"))?),
            "--y" => args.y = Some(value("--y")?.parse().map_err(|e| format!("--y: {e}"))?),
            "--list" => args.list = true,
            "--gm" => {
                let v = value("--gm")?;
                let (b, p) = v
                    .split_once(':')
                    .ok_or("--gm wants BANK:PROGRAM, e.g. 0:0")?;
                let b: u16 = b.parse().map_err(|e| format!("--gm bank: {e}"))?;
                let p: u16 = p.parse().map_err(|e| format!("--gm program: {e}"))?;
                if b > 128 || p > 127 {
                    return Err("--gm bank must be 0..=128 and program 0..=127".to_owned());
                }
                args.gm = Some((b, p));
            }
            "--gm-survey" => args.gm_survey = true,
            "-h" | "--help" => return Err(String::new()),
            other => return Err(format!("unknown argument '{other}'")),
        }
    }
    Ok(args)
}

fn spec_for(index: usize, args: &Args) -> RenderSpec {
    let mut spec = RenderSpec::new(index);
    if let Some(note) = args.note {
        spec.note = note;
    }
    if let Some(hold) = args.hold {
        spec.hold = hold.clamp(0.01, 60.0);
    }
    if let Some(x) = args.x {
        spec.pad_x = x.clamp(0.0, 1.0);
    }
    if let Some(y) = args.y {
        spec.pad_y = y.clamp(0.0, 1.0);
    }
    spec
}

fn header() {
    println!(
        "{:>2}  {:<14} {:<6} {:>4}  {:>8} {:>8} {:>9} {:>7} {:>7}",
        "#", "sound", "cat", "note", "peak dB", "RMS dB", "centroid", "tail s", "len s"
    );
}

fn row(index: usize, spec: &RenderSpec, r: &Rendered) {
    let p = &PRESETS[index];
    let octave = i32::from(spec.note / 12) - 1;
    println!(
        "{:>2}  {:<14} {:<6} {:>4}  {:>8.1} {:>8.1} {:>7.0}Hz {:>7.2} {:>7.2}",
        index + 1,
        p.name,
        p.category.name(),
        format!("{}{}", note_name_ascii(spec.note), octave),
        db(r.stats.peak),
        db(r.stats.rms),
        r.stats.centroid,
        r.stats.tail,
        r.stats.seconds,
    );
}

fn run() -> Result<(), String> {
    let args = parse_args()?;
    if args.gm.is_some() || args.gm_survey {
        let sf = hum_tools::render::load_soundfont()
            .map_err(|e| format!("cannot load the SoundFont: {e}"))?;
        let list: Vec<(u16, u16)> = match args.gm {
            Some(one) => vec![one],
            None => vec![
                (0, 0),
                (0, 4),
                (0, 11),
                (0, 19),
                (0, 24),
                (0, 33),
                (0, 40),
                (0, 48),
                (0, 52),
                (0, 56),
                (0, 65),
                (0, 73),
                (0, 80),
                (0, 88),
                (128, 0),
            ],
        };
        println!(
            "{:>9}  {:<24} {:>8} {:>8}",
            "bank:prg", "name", "peak dB", "RMS dB"
        );
        for (bank, program) in list {
            let note = if bank == 128 {
                38
            } else {
                args.note.unwrap_or(60)
            };
            let r =
                hum_tools::render::render_gm(&sf, bank, program, note, args.hold.unwrap_or(1.5));
            let name = sf
                .get_presets()
                .iter()
                .find(|p| {
                    p.get_bank_number() == i32::from(bank)
                        && p.get_patch_number() == i32::from(program)
                })
                .map_or("?", |p| p.get_name());
            println!(
                "{:>9}  {:<24} {:>8.1} {:>8.1}",
                format!("{bank}:{program}"),
                name,
                db(r.stats.peak),
                db(r.stats.rms)
            );
            if let Some(out) = &args.out {
                wav::write(out, &r.samples, 2, SAMPLE_RATE)
                    .map_err(|e| format!("cannot write {}: {e}", out.display()))?;
                println!("\nWrote {}", out.display());
            }
        }
        return Ok(());
    }
    if args.list {
        for (i, p) in PRESETS.iter().enumerate() {
            println!("{:>2}  {:<14} {}", i + 1, p.name, p.category.name());
        }
        return Ok(());
    }
    if args.all {
        let dir = args
            .outdir
            .clone()
            .unwrap_or_else(|| PathBuf::from("renders"));
        header();
        for index in 0..PRESETS.len() {
            let spec = spec_for(index, &args);
            let r = render(&spec);
            let path = dir.join(format!("{}.wav", file_stem(index)));
            wav::write(&path, &r.samples, 2, SAMPLE_RATE)
                .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
            row(index, &spec, &r);
        }
        println!("\nWrote {} files to {}", PRESETS.len(), dir.display());
        return Ok(());
    }
    let Some(name) = args.sound.as_deref() else {
        return Err("give --sound <NAME>, --all or --list".to_owned());
    };
    let index = presets::find(name).ok_or(format!(
        "no sound named '{name}' (use --list to see all 56)"
    ))?;
    let spec = spec_for(index, &args);
    let r = render(&spec);
    let path = args.out.clone().unwrap_or_else(|| PathBuf::from("out.wav"));
    wav::write(&path, &r.samples, 2, SAMPLE_RATE)
        .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    header();
    row(index, &spec, &r);
    println!("\nWrote {}", path.display());
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(msg) => {
            if !msg.is_empty() {
                eprintln!("error: {msg}\n");
            }
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}
