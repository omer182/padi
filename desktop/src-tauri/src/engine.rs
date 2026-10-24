// Built-in sound engine, rendered straight to the default audio output (cpal).
// Cross-platform, no external process. Signal chain per frame:
//
//   SoundFont synths (one per loaded .sf2) -> low-pass
//     + sampler voices (per-pad Robot / Lo-fi FX)
//     + echo bus (tempo-synced delay fed by pads and the synth)   = dry
//   dry -> (looper record) ; dry + looper layers -> volume -> session tap
//   + metronome click (never recorded) -> output
//
// Everything that touches audio state goes through `Cmd`s drained inside the
// audio callback, so MIDI/UI threads never block it. Big buffers (loop layers,
// synths) are built by the sender, never on the audio thread. The engine
// thread reopens the output when the stream fails or the default output changes.
use crate::hub::Hub;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample, StreamConfig};
use rustysynth::{SoundFont, Synthesizer, SynthesizerSettings};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum PlayMode {
    /// Plays to the end on every hit.
    Oneshot,
    /// Plays while the pad is held.
    Hold,
    /// Loops while the pad is held.
    Loop,
}

#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum VoiceFx {
    #[default]
    None,
    /// Ring modulation: the classic robot / dalek voice.
    Robot,
    /// Sample-rate + bit reduction: lo-fi toy sound.
    Crush,
}

/// A sample ready to play on a pad (mono, already reversed if wanted).
pub struct Slot {
    pub data: Arc<Vec<f32>>,
    pub sample_rate: u32,
    pub mode: PlayMode,
    pub gain: f32,
    /// Semitones.
    pub pitch: f32,
    /// Echo send, 0..1.
    pub echo: f32,
    pub fx: VoiceFx,
}

/// Per-channel sound shaping for SoundFont sounds (the sound editor).
#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SoundParams {
    pub reverb: u8,
    pub chorus: u8,
    /// Low-pass cutoff 0..127 (127 = open).
    pub brightness: u8,
    pub volume: u8,
    pub pan: u8,
    pub octave: i8,
    /// Echo send, 0..1.
    pub echo: f32,
}

impl Default for SoundParams {
    fn default() -> Self {
        SoundParams { reverb: 40, chorus: 0, brightness: 127, volume: 100, pan: 64, octave: 0, echo: 0.0 }
    }
}

pub enum Cmd {
    /// Raw channel message: status byte (incl. channel), data1, data2.
    Midi(u8, u8, u8),
    Program { ch: u8, font: usize, bank: i32, program: i32 },
    /// Add another SoundFont (synth already built for `rate`).
    AddFont(Arc<SoundFont>, Box<Synthesizer>, u32),
    Params(u8, SoundParams),
    /// Master low-pass cutoff, CC-style 0..127 (127 = open). The pad knob.
    Filter(u8),
    Volume(f32),
    Panic,
    SetSlot(u8, Option<Arc<Slot>>),
    PadOn(u8, u8),
    PadOff(u8),
    /// Record one loop cycle into this buffer (stereo interleaved, sized to the
    /// loop). Starts a new loop (after a one-bar count-in) if there are no layers.
    LooperRecord(Vec<f32>),
    LooperStop,
    LooperUndo,
    LooperRedo,
    LooperRemove(usize),
    LooperClear,
    LooperMute(usize, bool),
    LooperTempo { bpm: f32, bars: u32 },
    Metronome(bool),
    /// Stream the post-volume mix (stereo interleaved) to a recorder.
    Tap(Option<Sender<Vec<f32>>>),
    /// Reply with (loop length in frames, unmuted layers).
    Layers(Sender<(usize, Vec<Arc<Vec<f32>>>)>),
}

#[derive(Clone, Serialize)]
pub struct PresetInfo {
    pub font: usize,
    pub bank: i32,
    pub program: i32,
    pub name: String,
}

/// Looper state readable from any thread (UI ticker).
#[derive(Default)]
pub struct LoopShared {
    pub pos: AtomicUsize,
    pub len: AtomicUsize,
    pub recording: AtomicBool,
    pub countin: AtomicUsize,
    pub playing: AtomicBool,
    pub redo: AtomicUsize,
    /// Bumped whenever the layer list or mutes change.
    pub version: AtomicUsize,
    /// Mute flag per layer (the layer count is its length).
    pub mutes: Mutex<Vec<bool>>,
}

struct Voice {
    slot: Arc<Slot>,
    pad: u8,
    pos: f64,
    rate: f64,
    gain: f32,
    fade: f32,
    releasing: bool,
    /// Robot oscillator phase / crush hold state.
    phase: f32,
    held: f32,
    hold_left: u32,
}

const MAX_VOICES: usize = 48;

struct Layer {
    buf: Arc<Vec<f32>>,
    muted: bool,
}

struct Looper {
    len: usize,
    pos: usize,
    layers: Vec<Layer>,
    redo: Vec<(usize, Layer)>,
    rec: Option<Vec<f32>>,
    rec_written: usize,
    countin: usize,
    playing: bool,
    bpm: f32,
    bars: u32,
    metronome: bool,
    dirty: bool,
}

struct Click {
    env: f32,
    phase: f32,
    freq: f32,
}

struct Font {
    sf: Arc<SoundFont>,
    synth: Synthesizer,
    /// Callbacks since a channel last pointed at this font (keeps tails alive).
    idle: u32,
}

struct Inner {
    fonts: Vec<Font>,
    /// Which font each MIDI channel plays.
    ch_font: [usize; 16],
    sample_rate: u32,
    rx: Receiver<Cmd>,
    /// (font, bank, program) per channel, re-applied if the synths are rebuilt.
    programs: [(usize, i32, i32); 16],
    params: [SoundParams; 16],
    cutoff: u8,
    lp_coeff: f32,
    lp: [f32; 2],
    volume: f32,
    left: Vec<f32>,
    right: Vec<f32>,
    mix_l: Vec<f32>,
    mix_r: Vec<f32>,
    slots: [Option<Arc<Slot>>; 16],
    voices: Vec<Voice>,
    echo_buf: Vec<f32>,
    echo_pos: usize,
    looper: Looper,
    click: Click,
    tap: Option<Sender<Vec<f32>>>,
    shared: Arc<LoopShared>,
}

const ECHO_FEEDBACK: f32 = 0.45;
const ECHO_MAX_SECONDS: usize = 2;

impl Inner {
    fn set_sample_rate(&mut self, sr: u32) -> Result<(), String> {
        if sr == self.sample_rate {
            return Ok(());
        }
        let settings = SynthesizerSettings::new(sr as i32);
        for f in &mut self.fonts {
            f.synth = Synthesizer::new(&f.sf, &settings).map_err(|e| format!("{e:?}"))?;
        }
        self.sample_rate = sr;
        for ch in 0..16u8 {
            let (font, bank, program) = self.programs[ch as usize];
            self.apply_program(ch, font, bank, program);
            self.apply_params(ch);
        }
        self.update_filter();
        self.echo_buf = vec![0.0; sr as usize * ECHO_MAX_SECONDS];
        self.echo_pos = 0;
        // Loop layers were recorded at the old rate; they'd play wrong now.
        self.looper_clear();
        Ok(())
    }

    fn synth(&mut self, ch: u8) -> &mut Synthesizer {
        let i = self.ch_font[ch as usize].min(self.fonts.len() - 1);
        &mut self.fonts[i].synth
    }

    /// `bank` is the SoundFont bank (128 = drum kits). rustysynth adds 128 on
    /// the percussion channel itself, so subtract it there.
    fn apply_program(&mut self, ch: u8, font: usize, bank: i32, program: i32) {
        let font = font.min(self.fonts.len() - 1);
        if self.ch_font[ch as usize] != font {
            self.synth(ch).note_off_all_channel(ch as i32, false);
            self.ch_font[ch as usize] = font;
        }
        self.programs[ch as usize] = (font, bank, program);
        let sel = if ch == 9 { (bank - 128).max(0) } else { bank };
        let s = self.synth(ch);
        s.process_midi_message(ch as i32, 0xB0, 0x00, sel);
        s.process_midi_message(ch as i32, 0xC0, program, 0);
    }

    fn apply_params(&mut self, ch: u8) {
        let p = self.params[ch as usize];
        let s = self.synth(ch);
        s.process_midi_message(ch as i32, 0xB0, 91, p.reverb as i32);
        s.process_midi_message(ch as i32, 0xB0, 93, p.chorus as i32);
        s.process_midi_message(ch as i32, 0xB0, 7, p.volume as i32);
        s.process_midi_message(ch as i32, 0xB0, 10, p.pan as i32);
    }

    fn update_filter(&mut self) {
        // 0..127 -> 150 Hz .. 18 kHz on an exponential curve; 127 = bypass.
        let t = self.cutoff as f32 / 127.0;
        let fc = 150.0 * (18000.0f32 / 150.0).powf(t);
        self.lp_coeff = 1.0 - (-2.0 * std::f32::consts::PI * fc / self.sample_rate as f32).exp();
    }

    fn beat_frames(&self) -> usize {
        (self.sample_rate as f32 * 60.0 / self.looper.bpm.max(20.0)) as usize
    }

    fn looper_clear(&mut self) {
        let l = &mut self.looper;
        l.layers.clear();
        l.redo.clear();
        l.rec = None;
        l.playing = false;
        l.countin = 0;
        l.len = 0;
        l.pos = 0;
        l.dirty = true;
    }

    /// Keep the loop length while redo can bring layers back; stop playing.
    fn looper_settle(&mut self) {
        let l = &mut self.looper;
        if l.layers.is_empty() && l.rec.is_none() {
            l.playing = false;
            l.pos = 0;
            if l.redo.is_empty() {
                l.len = 0;
            }
        }
        l.dirty = true;
    }

    fn transpose(&self, ch: u8, note: u8) -> u8 {
        (note as i32 + self.params[ch as usize].octave as i32 * 12).clamp(0, 127) as u8
    }

    fn drain(&mut self) {
        while let Ok(cmd) = self.rx.try_recv() {
            match cmd {
                Cmd::Midi(status, d1, d2) => {
                    let ch = status & 0x0F;
                    let kind = (status & 0xF0) as i32;
                    let d1 = if kind == 0x80 || kind == 0x90 { self.transpose(ch, d1) } else { d1 };
                    if kind == 0xB0 && d1 == 0 {
                        self.programs[ch as usize].1 = d2 as i32 + if ch == 9 { 128 } else { 0 };
                    } else if kind == 0xC0 {
                        self.programs[ch as usize].2 = d1 as i32;
                    }
                    self.synth(ch).process_midi_message(ch as i32, kind, d1 as i32, d2 as i32);
                }
                Cmd::Program { ch, font, bank, program } => {
                    self.synth(ch).note_off_all_channel(ch as i32, false);
                    self.apply_program(ch, font, bank, program);
                    self.apply_params(ch);
                }
                Cmd::AddFont(sf, synth, rate) => {
                    // Output rate changed while it loaded: rebuild (rare).
                    let synth = if rate == self.sample_rate {
                        Some(*synth)
                    } else {
                        Synthesizer::new(&sf, &SynthesizerSettings::new(self.sample_rate as i32)).ok()
                    };
                    if let Some(synth) = synth {
                        self.fonts.push(Font { sf, synth, idle: 0 });
                    }
                }
                Cmd::Params(ch, p) => {
                    if p.octave != self.params[ch as usize].octave {
                        self.synth(ch).note_off_all_channel(ch as i32, false);
                    }
                    self.params[ch as usize] = p;
                    self.apply_params(ch);
                    if ch == 0 {
                        self.cutoff = p.brightness.min(127);
                        self.update_filter();
                    }
                }
                Cmd::Filter(v) => {
                    self.cutoff = v.min(127);
                    self.update_filter();
                }
                Cmd::Volume(v) => self.volume = v.clamp(0.0, 1.5),
                Cmd::Panic => {
                    for f in &mut self.fonts {
                        f.synth.note_off_all(true);
                    }
                    self.voices.clear();
                    self.echo_buf.fill(0.0);
                }
                Cmd::SetSlot(pad, slot) => {
                    if let Some(s) = self.slots.get_mut(pad as usize) {
                        *s = slot;
                    }
                    self.voices.retain(|v| v.pad != pad);
                }
                Cmd::PadOn(pad, vel) => self.pad_on(pad, vel),
                Cmd::PadOff(pad) => {
                    for v in self.voices.iter_mut().filter(|v| v.pad == pad) {
                        if v.slot.mode != PlayMode::Oneshot {
                            v.releasing = true;
                        }
                    }
                }
                Cmd::LooperRecord(buf) => {
                    let beat = self.beat_frames();
                    let l = &mut self.looper;
                    if l.rec.is_some() {
                        continue;
                    }
                    if l.layers.is_empty() {
                        // New loop: one bar of count-in, then record from the top.
                        l.len = buf.len() / 2;
                        l.pos = 0;
                        l.playing = false;
                        l.countin = beat * 4;
                        l.redo.clear();
                    } else if buf.len() == l.len * 2 {
                        // Overdub: record one full cycle from where we are.
                        l.playing = true;
                    } else {
                        continue;
                    }
                    l.rec = Some(buf);
                    l.rec_written = 0;
                }
                Cmd::LooperStop => {
                    let l = &mut self.looper;
                    l.rec = None;
                    l.countin = 0;
                    l.playing = !l.playing && !l.layers.is_empty();
                    l.pos = 0;
                }
                Cmd::LooperUndo => {
                    let l = &mut self.looper;
                    if l.rec.take().is_none() && l.countin == 0 {
                        if let Some(layer) = l.layers.pop() {
                            l.redo.push((l.layers.len(), layer));
                        }
                    }
                    l.countin = 0;
                    self.looper_settle();
                }
                Cmd::LooperRedo => {
                    let l = &mut self.looper;
                    if let Some((i, layer)) = l.redo.pop() {
                        let i = i.min(l.layers.len());
                        l.layers.insert(i, layer);
                        if !l.playing {
                            l.playing = true;
                            l.pos = 0;
                        }
                        l.dirty = true;
                    }
                }
                Cmd::LooperRemove(i) => {
                    let l = &mut self.looper;
                    if i < l.layers.len() {
                        let layer = l.layers.remove(i);
                        l.redo.push((i, layer));
                    }
                    self.looper_settle();
                }
                Cmd::LooperClear => self.looper_clear(),
                Cmd::LooperMute(i, m) => {
                    if let Some(layer) = self.looper.layers.get_mut(i) {
                        layer.muted = m;
                        self.looper.dirty = true;
                    }
                }
                Cmd::LooperTempo { bpm, bars } => {
                    if self.looper.layers.is_empty() && self.looper.rec.is_none() {
                        self.looper.bpm = bpm.clamp(40.0, 220.0);
                        self.looper.bars = bars.clamp(1, 16);
                    }
                }
                Cmd::Metronome(on) => self.looper.metronome = on,
                Cmd::Tap(tx) => self.tap = tx,
                Cmd::Layers(reply) => {
                    let layers = self.looper.layers.iter().filter(|l| !l.muted).map(|l| l.buf.clone()).collect();
                    let _ = reply.send((self.looper.len, layers));
                }
            }
        }
    }

    fn pad_on(&mut self, pad: u8, vel: u8) {
        let Some(slot) = self.slots.get(pad as usize).and_then(|s| s.clone()) else { return };
        self.voices.retain(|v| v.pad != pad); // retrigger restarts the pad
        if self.voices.len() >= MAX_VOICES {
            self.voices.remove(0);
        }
        let rate = slot.sample_rate as f64 / self.sample_rate as f64 * 2f64.powf(slot.pitch as f64 / 12.0);
        let gain = slot.gain * (vel.max(1) as f32 / 127.0).powf(0.6);
        self.voices.push(Voice { slot, pad, pos: 0.0, rate, gain, fade: 1.0, releasing: false, phase: 0.0, held: 0.0, hold_left: 0 });
    }

    /// Next sampler output (mono) and its echo send.
    fn voices_next(&mut self, fade_step: f32) -> (f32, f32) {
        let (mut out, mut send) = (0.0, 0.0);
        let robot_step = 55.0 / self.sample_rate as f32;
        self.voices.retain_mut(|v| {
            let d = &v.slot.data;
            let last = d.len().saturating_sub(1);
            if last == 0 {
                return false;
            }
            if v.pos >= last as f64 {
                if v.slot.mode == PlayMode::Loop && !v.releasing {
                    v.pos -= last as f64;
                } else {
                    return false;
                }
            }
            let i = v.pos as usize;
            let f = (v.pos - i as f64) as f32;
            let mut s = d[i] * (1.0 - f) + d[i + 1] * f;
            match v.slot.fx {
                VoiceFx::None => {}
                VoiceFx::Robot => {
                    s *= (v.phase * std::f32::consts::TAU).sin() * 1.6;
                    v.phase = (v.phase + robot_step).fract();
                }
                VoiceFx::Crush => {
                    if v.hold_left == 0 {
                        v.held = (s * 12.0).round() / 12.0;
                        v.hold_left = 6;
                    }
                    v.hold_left -= 1;
                    s = v.held;
                }
            }
            let s = s * v.gain * v.fade;
            out += s;
            send += s * v.slot.echo;
            v.pos += v.rate;
            if v.releasing {
                v.fade -= fade_step;
                return v.fade > 0.0;
            }
            true
        });
        (out, send)
    }

    fn click_trigger(&mut self, downbeat: bool) {
        self.click = Click { env: 1.0, phase: 0.0, freq: if downbeat { 1760.0 } else { 1180.0 } };
    }

    fn click_next(&mut self) -> f32 {
        if self.click.env < 0.001 {
            return 0.0;
        }
        let s = (self.click.phase * std::f32::consts::TAU).sin() * self.click.env * 0.3;
        self.click.phase = (self.click.phase + self.click.freq / self.sample_rate as f32).fract();
        self.click.env *= 0.9975;
        s
    }

    fn render_synths(&mut self, frames: usize) {
        if self.left.len() < frames {
            self.left.resize(frames, 0.0);
            self.right.resize(frames, 0.0);
            self.mix_l.resize(frames, 0.0);
            self.mix_r.resize(frames, 0.0);
        }
        self.mix_l[..frames].fill(0.0);
        self.mix_r[..frames].fill(0.0);
        for (i, f) in self.fonts.iter_mut().enumerate() {
            if self.ch_font.contains(&i) {
                f.idle = 0;
            } else {
                f.idle = f.idle.saturating_add(1);
                if f.idle > 2000 {
                    continue; // unused for a while: its tails are long gone
                }
            }
            f.synth.render(&mut self.left[..frames], &mut self.right[..frames]);
            for n in 0..frames {
                self.mix_l[n] += self.left[n];
                self.mix_r[n] += self.right[n];
            }
        }
    }

    fn render<T: SizedSample + FromSample<f32>>(&mut self, out: &mut [T], channels: usize) {
        self.drain();
        let frames = out.len() / channels;
        self.render_synths(frames);
        let bypass = self.cutoff >= 127;
        let fade_step = 1.0 / (0.012 * self.sample_rate as f32);
        let beat = self.beat_frames().max(1);
        // Echo time: a dotted eighth at the looper tempo.
        let echo_delay = ((beat * 3 / 4).max(1)).min(self.echo_buf.len().saturating_sub(1).max(1));
        let synth_echo = self.params[0].echo;
        let mut tap = self.tap.as_ref().map(|_| Vec::with_capacity(frames * 2));

        for (i, frame) in out.chunks_mut(channels).enumerate() {
            let (mut l, mut r) = (self.mix_l[i], self.mix_r[i]);
            if !bypass {
                self.lp[0] += self.lp_coeff * (l - self.lp[0]);
                self.lp[1] += self.lp_coeff * (r - self.lp[1]);
                l = self.lp[0];
                r = self.lp[1];
            } else {
                self.lp = [l, r];
            }
            let (s, pad_send) = self.voices_next(fade_step);
            let send = pad_send + (l + r) * 0.5 * synth_echo;
            l += s;
            r += s;

            if !self.echo_buf.is_empty() {
                let n = self.echo_buf.len();
                let read = (self.echo_pos + n - echo_delay) % n;
                let wet = self.echo_buf[read];
                self.echo_buf[self.echo_pos] = send + wet * ECHO_FEEDBACK;
                self.echo_pos = (self.echo_pos + 1) % n;
                l += wet * 0.7;
                r += wet * 0.7;
            }

            // Looper: count-in, record the dry signal, add the layers.
            let mut click = None;
            let lp = &mut self.looper;
            if lp.countin > 0 {
                let total = beat * 4;
                let elapsed = total - lp.countin;
                if elapsed % beat == 0 {
                    click = Some(elapsed == 0);
                }
                lp.countin -= 1;
                if lp.countin == 0 {
                    lp.playing = true;
                    lp.pos = 0;
                    lp.rec_written = 0;
                }
            }
            if lp.playing && lp.len > 0 {
                if lp.metronome && lp.pos % beat == 0 {
                    click = Some(lp.pos % (beat * 4) == 0);
                }
                if let Some(buf) = lp.rec.as_mut() {
                    buf[lp.pos * 2] = l;
                    buf[lp.pos * 2 + 1] = r;
                    lp.rec_written += 1;
                    if lp.rec_written >= lp.len {
                        let done = lp.rec.take().unwrap();
                        lp.layers.push(Layer { buf: Arc::new(done), muted: false });
                        lp.redo.clear();
                        lp.dirty = true;
                    }
                }
                for layer in &lp.layers {
                    if !layer.muted {
                        l += layer.buf[lp.pos * 2];
                        r += layer.buf[lp.pos * 2 + 1];
                    }
                }
                lp.pos = (lp.pos + 1) % lp.len;
            }
            if let Some(down) = click {
                self.click_trigger(down);
            }

            l *= self.volume;
            r *= self.volume;
            if let Some(t) = tap.as_mut() {
                t.push(l);
                t.push(r);
            }
            let c = self.click_next();
            let (ol, or) = (l + c, r + c);
            for (ch, smp) in frame.iter_mut().enumerate() {
                let v = match ch {
                    0 => ol,
                    1 => or,
                    _ => 0.0,
                };
                *smp = T::from_sample(v.clamp(-1.0, 1.0));
            }
        }

        if let (Some(t), Some(tx)) = (tap, self.tap.as_ref()) {
            if tx.send(t).is_err() {
                self.tap = None;
            }
        }
        self.publish();
    }

    fn publish(&mut self) {
        let lp = &mut self.looper;
        let sh = &self.shared;
        if lp.dirty {
            if let Ok(mut m) = sh.mutes.try_lock() {
                *m = lp.layers.iter().map(|l| l.muted).collect();
                lp.dirty = false;
                sh.version.fetch_add(1, Ordering::Relaxed);
            }
        }
        sh.pos.store(lp.pos, Ordering::Relaxed);
        sh.len.store(lp.len, Ordering::Relaxed);
        sh.redo.store(lp.redo.len(), Ordering::Relaxed);
        sh.recording.store(lp.rec.is_some(), Ordering::Relaxed);
        sh.countin.store(lp.countin, Ordering::Relaxed);
        sh.playing.store(lp.playing, Ordering::Relaxed);
    }
}

pub struct Engine {
    tx: Sender<Cmd>,
    presets: Mutex<Vec<PresetInfo>>,
    font_names: Mutex<Vec<String>>,
    sample_rate: Arc<AtomicU32>,
    pub looper: Arc<LoopShared>,
}

fn load_font(path: &Path) -> Result<Arc<SoundFont>, String> {
    let mut file = File::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
    Ok(Arc::new(SoundFont::new(&mut file).map_err(|e| format!("{}: {e:?}", path.display()))?))
}

fn font_presets(font: usize, sf: &SoundFont) -> Vec<PresetInfo> {
    let mut v: Vec<PresetInfo> = sf
        .get_presets()
        .iter()
        .map(|p| PresetInfo {
            font,
            bank: p.get_bank_number(),
            program: p.get_patch_number(),
            name: p.get_name().trim().to_string(),
        })
        .collect();
    v.sort_by_key(|p| (p.bank, p.program));
    v
}

impl Engine {
    /// Start with the main (GM) SoundFont; more can be added with `add_font`.
    pub fn start(hub: Arc<Hub>, sf_path: &Path, name: &str) -> Result<Arc<Engine>, String> {
        let sf = load_font(sf_path)?;
        let presets = font_presets(0, &sf);

        let (tx, rx) = channel();
        let initial_sr = 48000;
        let synth = Synthesizer::new(&sf, &SynthesizerSettings::new(initial_sr as i32))
            .map_err(|e| format!("{e:?}"))?;
        let mut programs = [(0, 0, 0); 16];
        programs[9] = (0, 128, 0); // GM drum channel
        let shared = Arc::new(LoopShared::default());
        let mut inner = Inner {
            fonts: vec![Font { sf, synth, idle: 0 }],
            ch_font: [0; 16],
            sample_rate: initial_sr,
            rx,
            programs,
            params: [SoundParams::default(); 16],
            cutoff: 127,
            lp_coeff: 1.0,
            lp: [0.0; 2],
            volume: hub.status().volume,
            left: Vec::new(),
            right: Vec::new(),
            mix_l: Vec::new(),
            mix_r: Vec::new(),
            slots: Default::default(),
            voices: Vec::with_capacity(MAX_VOICES),
            echo_buf: vec![0.0; initial_sr as usize * ECHO_MAX_SECONDS],
            echo_pos: 0,
            looper: Looper {
                len: 0,
                pos: 0,
                layers: Vec::new(),
                redo: Vec::new(),
                rec: None,
                rec_written: 0,
                countin: 0,
                playing: false,
                bpm: 100.0,
                bars: 2,
                metronome: true,
                dirty: true,
            },
            click: Click { env: 0.0, phase: 0.0, freq: 1000.0 },
            tap: None,
            shared: shared.clone(),
        };
        inner.update_filter();
        let inner = Arc::new(Mutex::new(inner));
        let sample_rate = Arc::new(AtomicU32::new(initial_sr));

        hub.log(format!("Sounds loaded: {name} ({} presets)", presets.len()));
        let h = hub.clone();
        let sr = sample_rate.clone();
        thread::Builder::new()
            .name("audio-out".into())
            .spawn(move || output_loop(h, inner, sr))
            .map_err(|e| e.to_string())?;

        Ok(Arc::new(Engine {
            tx,
            presets: Mutex::new(presets),
            font_names: Mutex::new(vec![name.to_string()]),
            sample_rate,
            looper: shared,
        }))
    }

    /// Load another SoundFont next to the main one. Its presets get `font` = its index.
    pub fn add_font(&self, path: &Path, name: &str) -> Result<usize, String> {
        let sf = load_font(path)?;
        let rate = self.sample_rate();
        let synth = Synthesizer::new(&sf, &SynthesizerSettings::new(rate as i32)).map_err(|e| format!("{e:?}"))?;
        let mut names = self.font_names.lock().unwrap();
        let font = names.len();
        names.push(name.to_string());
        self.presets.lock().unwrap().extend(font_presets(font, &sf));
        self.send(Cmd::AddFont(sf, Box::new(synth), rate));
        Ok(font)
    }

    pub fn send(&self, cmd: Cmd) {
        let _ = self.tx.send(cmd);
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate.load(Ordering::Relaxed)
    }

    pub fn presets(&self) -> Vec<PresetInfo> {
        self.presets.lock().unwrap().clone()
    }

    pub fn font_names(&self) -> Vec<String> {
        self.font_names.lock().unwrap().clone()
    }

    pub fn preset_name(&self, font: usize, bank: i32, program: i32) -> String {
        self.presets
            .lock()
            .unwrap()
            .iter()
            .find(|p| p.font == font && p.bank == bank && p.program == program)
            .map(|p| p.name.clone())
            .unwrap_or_else(|| format!("Bank {bank} / {program}"))
    }
}

/// Keeps an output stream open on the current default device, reopening it
/// when it errors out or the default device changes.
fn output_loop(hub: Arc<Hub>, inner: Arc<Mutex<Inner>>, rate: Arc<AtomicU32>) {
    let mut last_error: Option<String> = None;
    loop {
        let host = cpal::default_host();
        let Some(device) = host.default_output_device() else {
            report_error(&hub, &mut last_error, "No audio output device".into());
            thread::sleep(Duration::from_secs(2));
            continue;
        };
        let id = device.id().ok();
        let name = device
            .description()
            .map(|d| d.name().to_string())
            .unwrap_or_else(|_| "Audio output".into());

        let failed = Arc::new(AtomicBool::new(false));
        let stream = match open_stream(&device, inner.clone(), failed.clone()) {
            Ok(s) => s,
            Err(e) => {
                report_error(&hub, &mut last_error, format!("{name}: {e}"));
                thread::sleep(Duration::from_secs(2));
                continue;
            }
        };
        let sr = inner.lock().unwrap().sample_rate;
        rate.store(sr, Ordering::Relaxed);
        last_error = None;
        hub.log(format!("Audio out: {name} @ {sr} Hz"));
        hub.update(|s| {
            s.engine.ok = true;
            s.engine.output = name.clone();
            s.engine.sample_rate = sr;
            s.engine.error = None;
        });

        loop {
            thread::sleep(Duration::from_millis(1000));
            if failed.load(Ordering::Relaxed) {
                hub.log("Audio stream error, reopening output");
                break;
            }
            let now = cpal::default_host().default_output_device().and_then(|d| d.id().ok());
            if now != id {
                hub.log("Default audio output changed, switching");
                break;
            }
        }
        drop(stream);
        hub.update(|s| s.engine.ok = false);
    }
}

fn report_error(hub: &Hub, last: &mut Option<String>, msg: String) {
    if last.as_deref() != Some(msg.as_str()) {
        hub.log(format!("Audio: {msg}"));
        hub.update(|s| {
            s.engine.ok = false;
            s.engine.error = Some(msg.clone());
        });
        *last = Some(msg);
    }
}

fn open_stream(
    device: &cpal::Device,
    inner: Arc<Mutex<Inner>>,
    failed: Arc<AtomicBool>,
) -> Result<cpal::Stream, String> {
    let supported = device.default_output_config().map_err(|e| e.to_string())?;
    let format = supported.sample_format();
    let config: StreamConfig = supported.into();
    inner.lock().unwrap().set_sample_rate(config.sample_rate)?;
    let stream = match format {
        SampleFormat::F32 => build::<f32>(device, config, inner, failed),
        SampleFormat::I16 => build::<i16>(device, config, inner, failed),
        SampleFormat::I32 => build::<i32>(device, config, inner, failed),
        SampleFormat::U16 => build::<u16>(device, config, inner, failed),
        other => return Err(format!("unsupported sample format {other}")),
    }?;
    stream.play().map_err(|e| e.to_string())?;
    Ok(stream)
}

fn build<T: SizedSample + FromSample<f32>>(
    device: &cpal::Device,
    config: StreamConfig,
    inner: Arc<Mutex<Inner>>,
    failed: Arc<AtomicBool>,
) -> Result<cpal::Stream, String> {
    let channels = config.channels as usize;
    device
        .build_output_stream(
            config,
            move |data: &mut [T], _: &cpal::OutputCallbackInfo| match inner.try_lock() {
                Ok(mut g) => g.render(data, channels),
                Err(_) => data.fill(T::EQUILIBRIUM),
            },
            move |err: cpal::Error| {
                if err.kind() != cpal::ErrorKind::Xrun {
                    failed.store(true, Ordering::Relaxed);
                }
            },
            None,
        )
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: u32 = 48000;

    fn inner() -> (Inner, Sender<Cmd>) {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/resources/GeneralUser-GS.sf2");
        let sf = Arc::new(SoundFont::new(&mut File::open(path).unwrap()).unwrap());
        let synth = Synthesizer::new(&sf, &SynthesizerSettings::new(SR as i32)).unwrap();
        let (tx, rx) = channel();
        let i = Inner {
            fonts: vec![Font { sf, synth, idle: 0 }],
            ch_font: [0; 16],
            sample_rate: SR,
            rx,
            programs: [(0, 0, 0); 16],
            params: [SoundParams::default(); 16],
            cutoff: 127,
            lp_coeff: 1.0,
            lp: [0.0; 2],
            volume: 1.0,
            left: Vec::new(),
            right: Vec::new(),
            mix_l: Vec::new(),
            mix_r: Vec::new(),
            slots: Default::default(),
            voices: Vec::new(),
            echo_buf: vec![0.0; SR as usize * ECHO_MAX_SECONDS],
            echo_pos: 0,
            looper: Looper {
                len: 0,
                pos: 0,
                layers: Vec::new(),
                redo: Vec::new(),
                rec: None,
                rec_written: 0,
                countin: 0,
                playing: false,
                bpm: 120.0,
                bars: 1,
                metronome: false,
                dirty: false,
            },
            click: Click { env: 0.0, phase: 0.0, freq: 1000.0 },
            tap: None,
            shared: Arc::new(LoopShared::default()),
        };
        (i, tx)
    }

    fn tone(secs: f32) -> Arc<Vec<f32>> {
        Arc::new((0..(SR as f32 * secs) as usize).map(|n| (n as f32 * 0.13).sin() * 0.5).collect())
    }

    /// Render `frames` stereo frames; returns the peak.
    fn run(i: &mut Inner, frames: usize) -> f32 {
        let mut peak = 0f32;
        let mut left = frames;
        while left > 0 {
            let n = left.min(480);
            let mut buf = vec![0f32; n * 2];
            i.render(&mut buf, 2);
            peak = buf.iter().fold(peak, |p, s| p.max(s.abs()));
            left -= n;
        }
        peak
    }

    fn slot_with(mode: PlayMode, echo: f32, fx: VoiceFx) -> Option<Arc<Slot>> {
        Some(Arc::new(Slot { data: tone(0.1), sample_rate: SR, mode, gain: 1.0, pitch: 0.0, echo, fx }))
    }

    fn slot(mode: PlayMode) -> Option<Arc<Slot>> {
        slot_with(mode, 0.0, VoiceFx::None)
    }

    #[test]
    fn oneshot_plays_then_ends() {
        let (mut i, tx) = inner();
        tx.send(Cmd::SetSlot(3, slot(PlayMode::Oneshot))).unwrap();
        tx.send(Cmd::PadOn(3, 127)).unwrap();
        tx.send(Cmd::PadOff(3)).unwrap(); // one-shot ignores release
        assert!(run(&mut i, 2400) > 0.3);
        run(&mut i, 4800);
        assert!(i.voices.is_empty());
        assert!(run(&mut i, 480) < 1e-4);
    }

    #[test]
    fn hold_stops_on_release_loop_sustains() {
        let (mut i, tx) = inner();
        tx.send(Cmd::SetSlot(0, slot(PlayMode::Hold))).unwrap();
        tx.send(Cmd::PadOn(0, 127)).unwrap();
        run(&mut i, 960);
        tx.send(Cmd::PadOff(0)).unwrap();
        run(&mut i, 960);
        assert!(i.voices.is_empty(), "hold: released before the sample ended");

        tx.send(Cmd::SetSlot(0, slot(PlayMode::Loop))).unwrap();
        tx.send(Cmd::PadOn(0, 127)).unwrap();
        run(&mut i, SR as usize); // 1s, well past the 0.1s sample
        assert!(run(&mut i, 480) > 0.3, "still looping");
        tx.send(Cmd::PadOff(0)).unwrap();
        run(&mut i, 2400);
        assert!(i.voices.is_empty(), "faded out after release");
    }

    #[test]
    fn echo_rings_after_the_sample_ends() {
        let (mut i, tx) = inner();
        tx.send(Cmd::SetSlot(0, slot_with(PlayMode::Oneshot, 1.0, VoiceFx::None))).unwrap();
        tx.send(Cmd::PadOn(0, 127)).unwrap();
        run(&mut i, 4800); // the 0.1s sample is over
        assert!(i.voices.is_empty());
        // Echo comes back a dotted eighth later (0.375s at 120 BPM).
        assert!(run(&mut i, 18000) > 0.1, "echo repeat audible");
    }

    #[test]
    fn robot_and_crush_change_the_sound() {
        let (mut i, tx) = inner();
        for fx in [VoiceFx::Robot, VoiceFx::Crush] {
            tx.send(Cmd::SetSlot(1, slot_with(PlayMode::Oneshot, 0.0, fx))).unwrap();
            tx.send(Cmd::PadOn(1, 127)).unwrap();
            assert!(run(&mut i, 2400) > 0.05, "{fx:?} still makes sound");
            run(&mut i, 4800);
        }
    }

    #[test]
    fn looper_counts_in_records_and_overdubs() {
        let (mut i, tx) = inner();
        let len = (SR as f32 * 60.0 / 120.0 * 4.0) as usize; // 1 bar @120 = 2s
        tx.send(Cmd::SetSlot(0, slot(PlayMode::Loop))).unwrap();
        tx.send(Cmd::LooperRecord(vec![0.0; len * 2])).unwrap();
        run(&mut i, 480);
        assert_eq!(i.looper.countin, len - 480, "one bar of count-in");
        assert!(!i.looper.playing);
        run(&mut i, len - 480);
        assert!(i.looper.playing && i.looper.rec.is_some(), "recording starts after count-in");

        // Hold a looping pad for the first half of the take.
        tx.send(Cmd::PadOn(0, 127)).unwrap();
        run(&mut i, len / 2);
        tx.send(Cmd::PadOff(0)).unwrap();
        run(&mut i, len / 2);
        assert_eq!(i.looper.layers.len(), 1, "take became a layer after one cycle");
        assert!(i.looper.rec.is_none());

        // Pad is silent now, so whatever we hear in the first half is the loop.
        assert!(run(&mut i, len / 4) > 0.3, "layer plays back");
        run(&mut i, len / 2 + len / 8);
        assert!(run(&mut i, len / 16) < 1e-3, "quiet half of the loop stays quiet");

        // Overdub records exactly one more cycle.
        let to_top = len - i.looper.pos;
        run(&mut i, to_top);
        tx.send(Cmd::LooperRecord(vec![0.0; len * 2])).unwrap();
        run(&mut i, len);
        assert_eq!(i.looper.layers.len(), 2);

        // Undo, redo, remove a specific layer, redo it back.
        tx.send(Cmd::LooperUndo).unwrap();
        run(&mut i, 480);
        assert_eq!((i.looper.layers.len(), i.looper.redo.len()), (1, 1));
        tx.send(Cmd::LooperRedo).unwrap();
        run(&mut i, 480);
        assert_eq!((i.looper.layers.len(), i.looper.redo.len()), (2, 0));
        tx.send(Cmd::LooperRemove(0)).unwrap();
        tx.send(Cmd::LooperRemove(0)).unwrap();
        run(&mut i, 480);
        assert!(i.looper.layers.is_empty() && !i.looper.playing && i.looper.len == len, "redo keeps the loop length");
        tx.send(Cmd::LooperRedo).unwrap();
        run(&mut i, 480);
        assert!(i.looper.playing && i.looper.layers.len() == 1);
        tx.send(Cmd::LooperClear).unwrap();
        run(&mut i, 480);
        assert_eq!((i.looper.len, i.looper.playing, i.looper.redo.len()), (0, false, 0));
    }

    #[test]
    fn octave_param_transposes_notes() {
        let (mut i, tx) = inner();
        tx.send(Cmd::Params(0, SoundParams { octave: 1, ..Default::default() })).unwrap();
        run(&mut i, 48);
        assert_eq!(i.transpose(0, 60), 72);
        assert_eq!(i.transpose(1, 60), 60, "other channels untouched");
        assert_eq!(i.transpose(0, 125), 127, "clamped");
    }
}
