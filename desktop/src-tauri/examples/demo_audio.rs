// Renders the README demo video's soundtrack: the same riff as demo.ts
// (VIDEO_RIFF, 96 BPM eighths) on the Spanish Guitar pack, with GeneralUser
// drums from loop 2 and bass from loop 3. Usage: cargo run --example demo_audio -- out.wav
use rustysynth::{SoundFont, Synthesizer, SynthesizerSettings};
use std::{fs::File, sync::Arc};

const SR: usize = 48000;
const BPM: f64 = 96.0;
const SECONDS: f64 = 15.0;
const RIFF: [&[usize]; 16] = [
    &[0, 7], &[4], &[9], &[4], &[5, 12], &[9], &[7], &[9],
    &[3, 10], &[7], &[12], &[7], &[4, 11], &[8], &[13], &[14],
];
const MAJOR: [i32; 16] = [0, 2, 4, 5, 7, 9, 11, 12, 14, 16, 17, 19, 21, 23, 24, 26];

struct Ev { t: f64, ch: i32, key: i32, vel: i32 } // vel 0 = note off

fn synth(path: &str) -> Synthesizer {
    let sf = Arc::new(SoundFont::new(&mut File::open(path).unwrap()).unwrap());
    Synthesizer::new(&sf, &SynthesizerSettings::new(SR as i32)).unwrap()
}

fn render(s: &mut Synthesizer, mut evs: Vec<Ev>, n: usize) -> (Vec<f32>, Vec<f32>) {
    evs.sort_by(|a, b| a.t.partial_cmp(&b.t).unwrap());
    let (mut l, mut r) = (vec![0f32; n], vec![0f32; n]);
    let mut pos = 0;
    let mut i = 0;
    while pos < n {
        let next = evs.get(i).map(|e| ((e.t * SR as f64) as usize).min(n)).unwrap_or(n);
        if next > pos {
            s.render(&mut l[pos..next], &mut r[pos..next]);
            pos = next;
        }
        while i < evs.len() && ((evs[i].t * SR as f64) as usize) <= pos {
            let e = &evs[i];
            if e.vel > 0 { s.note_on(e.ch, e.key, e.vel) } else { s.note_off(e.ch, e.key) }
            i += 1;
        }
        if i >= evs.len() && pos < n {
            s.render(&mut l[pos..], &mut r[pos..]);
            pos = n;
        }
    }
    (l, r)
}

fn main() {
    let out = std::env::args().nth(1).unwrap_or("demo.wav".into());
    let step = 60.0 / BPM / 2.0;
    let hold = 0.25;
    let n = (SECONDS * SR as f64) as usize;
    let steps = (SECONDS / step) as usize;

    // Guitar: the riff, every step.
    let mut gtr = synth("resources/packs/Spanish Guitar.sf2");
    gtr.process_midi_message(0, 0xB0, 91, 75);
    gtr.process_midi_message(0, 0xB0, 93, 25);
    let mut g = Vec::new();
    for k in 0..steps {
        let t = k as f64 * step;
        for (j, &p) in RIFF[k % 16].iter().enumerate() {
            let key = 48 + MAJOR[p];
            g.push(Ev { t, ch: 0, key, vel: if j == 0 { 96 } else { 104 } });
            g.push(Ev { t: t + hold, ch: 0, key, vel: 0 });
        }
    }

    // Drums (from loop 2) + bass (from loop 3) on GeneralUser.
    let mut gm = synth("resources/GeneralUser-GS.sf2");
    gm.process_midi_message(1, 0xC0, 33, 0); // finger bass
    gm.process_midi_message(9, 0xB0, 91, 30);
    let mut d = Vec::new();
    let kick = [0, 5, 8, 10];
    let snare = [4, 12];
    for k in 16..steps {
        let t = k as f64 * step;
        let s = k % 16;
        let mut hit = |key: i32, vel: i32| {
            d.push(Ev { t, ch: 9, key, vel });
            d.push(Ev { t: t + 0.1, ch: 9, key, vel: 0 });
        };
        if kick.contains(&s) { hit(36, 112) }
        if snare.contains(&s) { hit(38, 100) }
        hit(if s == 15 { 46 } else { 42 }, if s % 2 == 0 { 70 } else { 48 });
    }
    let roots = [36, 33, 29, 31]; // C, A, F, G: one per half bar
    for k in (32..steps).step_by(4) {
        let t = k as f64 * step;
        let key = roots[(k % 16) / 4];
        d.push(Ev { t, ch: 1, key, vel: 92 });
        d.push(Ev { t: t + step * 3.6, ch: 1, key, vel: 0 });
    }

    let (gl, gr) = render(&mut gtr, g, n);
    let (dl, dr) = render(&mut gm, d, n);

    // Dotted-eighth ping-pong echo on the guitar (like PADI's Echo).
    let delay = (step * 1.5 * SR as f64) as usize;
    let (mut el, mut er) = (vec![0f32; n], vec![0f32; n]);
    for i in delay..n {
        el[i] = gr[i - delay] * 0.3 + er[i - delay] * 0.4;
        er[i] = gl[i - delay] * 0.3 + el[i - delay] * 0.4;
    }

    let mut mix: Vec<f32> = Vec::with_capacity(n * 2);
    for i in 0..n {
        mix.push(gl[i] + el[i] * 0.6 + dl[i] * 0.85);
        mix.push(gr[i] + er[i] * 0.6 + dr[i] * 0.85);
    }
    let peak = mix.iter().fold(0f32, |m, v| m.max(v.abs()));
    let gain = 0.89 / peak;
    let fade = (0.8 * SR as f64) as usize;
    let spec = hound::WavSpec { channels: 2, sample_rate: SR as u32, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut w = hound::WavWriter::create(&out, spec).unwrap();
    for (i, s) in mix.iter().enumerate() {
        let f = i / 2;
        let env = if f > n - fade { (n - f) as f32 / fade as f32 } else { 1.0 };
        w.write_sample((s * gain * env * i16::MAX as f32) as i16).unwrap();
    }
    w.finalize().unwrap();
    println!("wrote {out} ({SECONDS}s, peak {peak:.3})");
}
