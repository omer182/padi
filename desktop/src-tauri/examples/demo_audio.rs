// Renders the README demo video's soundtrack from scripts/demo-video/timeline.json
// (the same timeline the picture is captured from).
//   cargo run --release --example demo_audio -- <timeline.json> <out.wav>
//
// Tracks: lead (GM Saw Lead, filter + echo automated like the pad knob and the
// editor's Echo slider), drums (GM 808/909 kit), bass (a synthesized 808 sub:
// sine with a pitch punch and glide), robot (GM Synth Voice ring-modulated at
// 55 Hz, like PADI's Robot voice), chip / hit / pop (GM choir, orchestra hit,
// woodblock), riser (SciFi FX pack, faded in).
use rustysynth::{SoundFont, Synthesizer, SynthesizerSettings};
use serde_json::Value;
use std::f32::consts::TAU;
use std::{fs::File, sync::Arc};

const SR: usize = 48000;

struct Note { t: f64, d: f64, k: i32, v: i32 }

fn notes(tl: &Value, track: &str) -> Vec<Note> {
    tl["tracks"][track].as_array().unwrap().iter().map(|n| Note {
        t: n["t"].as_f64().unwrap(), d: n["d"].as_f64().unwrap(),
        k: n["k"].as_i64().unwrap() as i32, v: n["v"].as_i64().unwrap() as i32,
    }).collect()
}

/// Piecewise-linear automation lane at time t.
fn lane(tl: &Value, name: &str, t: f64) -> f32 {
    let pts: Vec<(f64, f64)> = tl["auto"][name].as_array().unwrap().iter()
        .map(|p| (p[0].as_f64().unwrap(), p[1].as_f64().unwrap())).collect();
    for w in pts.windows(2) {
        if t <= w[1].0 {
            let f = if w[1].0 > w[0].0 { ((t - w[0].0) / (w[1].0 - w[0].0)).clamp(0.0, 1.0) } else { 1.0 };
            return (w[0].1 + (w[1].1 - w[0].1) * f) as f32;
        }
    }
    pts.last().unwrap().1 as f32
}

fn font(path: &str) -> Arc<SoundFont> {
    Arc::new(SoundFont::new(&mut File::open(path).unwrap()).unwrap())
}

/// Play `ns` on one channel of a fresh synth; `setup` sends program/CCs first.
fn play(sf: &Arc<SoundFont>, ch: i32, setup: &[(i32, i32, i32)], ns: &[Note], n: usize) -> (Vec<f32>, Vec<f32>) {
    let mut s = Synthesizer::new(sf, &SynthesizerSettings::new(SR as i32)).unwrap();
    for &(cmd, a, b) in setup { s.process_midi_message(ch, cmd, a, b); }
    let mut evs: Vec<(usize, bool, i32, i32)> = Vec::new();
    for x in ns {
        evs.push(((x.t * SR as f64) as usize, true, x.k, x.v.clamp(1, 127)));
        evs.push((((x.t + x.d) * SR as f64) as usize, false, x.k, 0));
    }
    evs.sort_by_key(|e| (e.0, e.1)); // offs before ons at the same time
    let (mut l, mut r) = (vec![0f32; n], vec![0f32; n]);
    let mut pos = 0;
    for (at, on, k, v) in evs {
        let at = at.min(n);
        if at > pos { s.render(&mut l[pos..at], &mut r[pos..at]); pos = at; }
        if on { s.note_on(ch, k, v) } else { s.note_off(ch, k) }
    }
    if pos < n { s.render(&mut l[pos..], &mut r[pos..]); }
    (l, r)
}

/// The 808: sine sub with a fast pitch drop at the hit, optional glide up
/// (`v` = semitones) over the note, long decay, a little saturation.
fn sub808(ns: &[Note], n: usize) -> Vec<f32> {
    let mut out = vec![0f32; n];
    for x in ns {
        let start = (x.t * SR as f64) as usize;
        let len = ((x.d + 0.35) * SR as f64) as usize;
        let base = 440.0 * 2f32.powf((x.k - 69) as f32 / 12.0);
        let mut phase = 0f32;
        for i in 0..len.min(n.saturating_sub(start)) {
            let t = i as f32 / SR as f32;
            let glide = if x.v > 0 { (t / x.d as f32).clamp(0.0, 1.0).powf(2.0) * x.v as f32 } else { 0.0 };
            let punch = 14.0 * (-t * 38.0).exp(); // semitones down to the note
            let f = base * 2f32.powf((glide + punch) / 12.0);
            phase = (phase + f / SR as f32).fract();
            let env = (t * 400.0).min(1.0) * (-t * 1.7).exp() * if t > x.d as f32 { (-(t - x.d as f32) * 18.0).exp() } else { 1.0 };
            let s = (phase * TAU).sin();
            out[start + i] += (s * 1.8).tanh() * env * 0.8;
        }
    }
    out
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let tl: Value = serde_json::from_reader(File::open(&args[1]).unwrap()).unwrap();
    let out = &args[2];
    let secs = tl["seconds"].as_f64().unwrap();
    let s16 = tl["s16"].as_f64().unwrap();
    let n = ((secs + 1.5) * SR as f64) as usize; // room for the echo tail

    let gm = font("resources/GeneralUser-GS.sf2");
    let scifi = font("resources/packs/SciFi FX.sf2");
    const PC: i32 = 0xC0;
    const CC: i32 = 0xB0;

    // Lead + its filter (the knob) and dotted-8th echo (the editor's Echo slider).
    let (mut ll, mut lr) = play(&gm, 0, &[(PC, 81, 0), (CC, 91, 55), (CC, 93, 45)], &notes(&tl, "lead"), n);
    let mut lp = [0f32; 2];
    for i in 0..n {
        let t = i as f64 / SR as f64;
        let c = lane(&tl, "cutoff", t);
        let fc = 180.0 * (16000.0f32 / 180.0).powf(c);
        let a = 1.0 - (-TAU * fc / SR as f32).exp();
        lp[0] += a * (ll[i] - lp[0]);
        lp[1] += a * (lr[i] - lp[1]);
        ll[i] = lp[0];
        lr[i] = lp[1];
    }
    let delay = (s16 * 3.0 * SR as f64) as usize;
    let (mut el, mut er) = (vec![0f32; n], vec![0f32; n]);
    for i in delay..n {
        let send = lane(&tl, "echo", i as f64 / SR as f64) * 1.6;
        el[i] = lr[i - delay] * send + er[i - delay] * 0.45;
        er[i] = ll[i - delay] * send + el[i - delay] * 0.45;
    }

    let (dl, dr) = play(&gm, 9, &[(CC, 0, 0), (PC, 25, 0), (CC, 91, 20)], &notes(&tl, "drums"), n);
    let bass = sub808(&notes(&tl, "bass"), n);
    let (rl, rr) = play(&gm, 2, &[(PC, 54, 0), (CC, 91, 40)], &notes(&tl, "robot"), n);
    let (cl, cr) = play(&gm, 3, &[(PC, 52, 0), (CC, 91, 60)], &notes(&tl, "chip"), n);
    let (hl, hr) = play(&gm, 4, &[(PC, 55, 0), (CC, 91, 50)], &notes(&tl, "hit"), n);
    let (pl, pr) = play(&gm, 5, &[(PC, 115, 0)], &notes(&tl, "pop"), n);
    let risers = notes(&tl, "riser");
    let (sl, sr) = play(&scifi, 0, &[], &risers, n);

    if std::env::var("STEMS").is_ok() {
        let b = (s16 * 16.0 * SR as f64) as usize;
        let rms = |x: &[f32]| (x[6 * b..10 * b].iter().map(|v| v * v).sum::<f32>() / (4 * b) as f32).sqrt();
        eprintln!("bars 6-9 rms: lead {:.4} drums {:.4} bass {:.4} robot {:.4} chip {:.4} hit {:.4} pop {:.4}",
            rms(&ll), rms(&dl), rms(&bass), rms(&rl), rms(&cl), rms(&hl), rms(&pl));
    }
    let mut mix = Vec::with_capacity(n * 2);
    let mut robot_phase = 0f32;
    for i in 0..n {
        let t = i as f64 / SR as f64;
        // Robot voice: ring modulation at 55 Hz, as in PADI.
        robot_phase = (robot_phase + 55.0 / SR as f32).fract();
        let ring = (robot_phase * TAU).sin() * 1.7;
        // Risers swell in over their note.
        let swell = risers.iter().find(|r| t >= r.t && t < r.t + r.d).map(|r| (((t - r.t) / r.d) as f32).powf(2.0)).unwrap_or(0.0);
        // Light sidechain: the lead ducks a touch under the 808.
        let duck = 1.0 - (bass[i].abs() * 0.35).min(0.3);
        let l = (ll[i] + el[i] * 0.7) * 2.6 * duck + dl[i] * 1.5 + bass[i] * 0.16 + rl[i] * ring * 3.2 + cl[i] * 6.0
            + hl[i] * 2.2 + pl[i] * 5.0 + sl[i] * swell * 1.4;
        let r = (lr[i] + er[i] * 0.7) * 2.6 * duck + dr[i] * 1.5 + bass[i] * 0.16 + rr[i] * ring * 3.2 + cr[i] * 6.0
            + hr[i] * 2.2 + pr[i] * 5.0 + sr[i] * swell * 1.4;
        mix.push(l);
        mix.push(r);
    }
    // Normalize, then a soft limiter for punch.
    let peak = mix.iter().fold(0f32, |m, v| m.max(v.abs()));
    let drive = 1.3 / peak;
    let norm = (1.3f32).tanh();
    let total = (secs * SR as f64) as usize;
    let fade = (0.6 * SR as f64) as usize;
    let spec = hound::WavSpec { channels: 2, sample_rate: SR as u32, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut w = hound::WavWriter::create(out, spec).unwrap();
    for (i, s) in mix.iter().take(total * 2).enumerate() {
        let f = i / 2;
        let env = if f > total - fade { (total - f) as f32 / fade as f32 } else { 1.0 };
        let y = (s * drive).tanh() / norm * 0.92 * env;
        w.write_sample((y * i16::MAX as f32) as i16).unwrap();
    }
    w.finalize().unwrap();
    println!("wrote {out}: {secs:.2}s");
}
