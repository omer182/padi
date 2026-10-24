// Built-in sound engine: a SoundFont synth (rustysynth) rendered straight to
// the default audio output (cpal). Cross-platform, no external process.
//
// Everything that touches the synth goes through `Cmd`s drained inside the
// audio callback, so MIDI/UI threads never block the audio thread. The engine
// thread reopens the output when the stream fails or the OS default output
// changes (headphones plugged in, etc.).
use crate::hub::Hub;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample, StreamConfig};
use rustysynth::{SoundFont, Synthesizer, SynthesizerSettings};
use serde::Serialize;
use std::fs::File;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

pub enum Cmd {
    /// Raw channel message: status byte (incl. channel), data1, data2.
    Midi(u8, u8, u8),
    Program { ch: u8, bank: i32, program: i32 },
    /// Master low-pass cutoff, CC-style 0..127 (127 = open).
    Filter(u8),
    Volume(f32),
    Panic,
}

#[derive(Clone, Serialize)]
pub struct PresetInfo {
    pub bank: i32,
    pub program: i32,
    pub name: String,
}

struct Inner {
    sf: Arc<SoundFont>,
    synth: Synthesizer,
    sample_rate: u32,
    rx: Receiver<Cmd>,
    /// (bank, program) per channel, re-applied if the synth is rebuilt.
    programs: [(i32, i32); 16],
    cutoff: u8,
    lp_coeff: f32,
    lp: [f32; 2],
    volume: f32,
    left: Vec<f32>,
    right: Vec<f32>,
}

impl Inner {
    fn set_sample_rate(&mut self, sr: u32) -> Result<(), String> {
        if sr == self.sample_rate {
            return Ok(());
        }
        let settings = SynthesizerSettings::new(sr as i32);
        self.synth = Synthesizer::new(&self.sf, &settings).map_err(|e| format!("{e:?}"))?;
        self.sample_rate = sr;
        for (ch, &(bank, program)) in self.programs.clone().iter().enumerate() {
            self.apply_program(ch as u8, bank, program);
        }
        self.update_filter();
        Ok(())
    }

    /// `bank` is the SoundFont bank (128 = drum kits). rustysynth adds 128 on
    /// the percussion channel itself, so subtract it there.
    fn apply_program(&mut self, ch: u8, bank: i32, program: i32) {
        self.programs[ch as usize] = (bank, program);
        let sel = if ch == 9 { (bank - 128).max(0) } else { bank };
        self.synth.process_midi_message(ch as i32, 0xB0, 0x00, sel);
        self.synth.process_midi_message(ch as i32, 0xC0, program, 0);
    }

    fn update_filter(&mut self) {
        // 0..127 -> 150 Hz .. 18 kHz on an exponential curve; 127 = bypass.
        let t = self.cutoff as f32 / 127.0;
        let fc = 150.0 * (18000.0f32 / 150.0).powf(t);
        self.lp_coeff = 1.0 - (-2.0 * std::f32::consts::PI * fc / self.sample_rate as f32).exp();
    }

    fn drain(&mut self) {
        while let Ok(cmd) = self.rx.try_recv() {
            match cmd {
                Cmd::Midi(status, d1, d2) => {
                    let ch = (status & 0x0F) as i32;
                    let kind = (status & 0xF0) as i32;
                    if kind == 0xB0 && d1 == 0 {
                        self.programs[ch as usize].0 = d2 as i32 + if ch == 9 { 128 } else { 0 };
                    } else if kind == 0xC0 {
                        self.programs[ch as usize].1 = d1 as i32;
                    }
                    self.synth.process_midi_message(ch, kind, d1 as i32, d2 as i32);
                }
                Cmd::Program { ch, bank, program } => {
                    self.synth.note_off_all_channel(ch as i32, false);
                    self.apply_program(ch, bank, program);
                }
                Cmd::Filter(v) => {
                    self.cutoff = v.min(127);
                    self.update_filter();
                }
                Cmd::Volume(v) => self.volume = v.clamp(0.0, 1.5),
                Cmd::Panic => self.synth.note_off_all(true),
            }
        }
    }

    fn render<T: SizedSample + FromSample<f32>>(&mut self, out: &mut [T], channels: usize) {
        self.drain();
        let frames = out.len() / channels;
        if self.left.len() < frames {
            self.left.resize(frames, 0.0);
            self.right.resize(frames, 0.0);
        }
        self.synth.render(&mut self.left[..frames], &mut self.right[..frames]);
        let bypass = self.cutoff >= 127;
        for (i, frame) in out.chunks_mut(channels).enumerate() {
            let (mut l, mut r) = (self.left[i], self.right[i]);
            if !bypass {
                self.lp[0] += self.lp_coeff * (l - self.lp[0]);
                self.lp[1] += self.lp_coeff * (r - self.lp[1]);
                l = self.lp[0];
                r = self.lp[1];
            } else {
                self.lp = [l, r];
            }
            l *= self.volume;
            r *= self.volume;
            for (c, s) in frame.iter_mut().enumerate() {
                let v = match c {
                    0 => l,
                    1 => r,
                    _ => 0.0,
                };
                *s = T::from_sample(v.clamp(-1.0, 1.0));
            }
        }
    }
}

pub struct Engine {
    tx: Sender<Cmd>,
    presets: Vec<PresetInfo>,
}

impl Engine {
    pub fn start(hub: Arc<Hub>, sf_path: &Path) -> Result<Arc<Engine>, String> {
        let mut file = File::open(sf_path).map_err(|e| format!("open {}: {e}", sf_path.display()))?;
        let sf = Arc::new(SoundFont::new(&mut file).map_err(|e| format!("SoundFont: {e:?}"))?);
        let mut presets: Vec<PresetInfo> = sf
            .get_presets()
            .iter()
            .map(|p| PresetInfo {
                bank: p.get_bank_number(),
                program: p.get_patch_number(),
                name: p.get_name().trim().to_string(),
            })
            .collect();
        presets.sort_by_key(|p| (p.bank, p.program));

        let (tx, rx) = channel();
        let initial_sr = 48000;
        let synth = Synthesizer::new(&sf, &SynthesizerSettings::new(initial_sr as i32))
            .map_err(|e| format!("{e:?}"))?;
        let mut programs = [(0, 0); 16];
        programs[9] = (128, 0); // GM drum channel
        let mut inner = Inner {
            sf,
            synth,
            sample_rate: initial_sr,
            rx,
            programs,
            cutoff: 127,
            lp_coeff: 1.0,
            lp: [0.0; 2],
            volume: hub.status().volume,
            left: Vec::new(),
            right: Vec::new(),
        };
        inner.update_filter();
        let inner = Arc::new(Mutex::new(inner));

        hub.log(format!("SoundFont loaded: {} presets", presets.len()));
        let h = hub.clone();
        thread::Builder::new()
            .name("audio-out".into())
            .spawn(move || output_loop(h, inner))
            .map_err(|e| e.to_string())?;

        Ok(Arc::new(Engine { tx, presets }))
    }

    pub fn send(&self, cmd: Cmd) {
        let _ = self.tx.send(cmd);
    }

    pub fn presets(&self) -> &[PresetInfo] {
        &self.presets
    }

    pub fn preset_name(&self, bank: i32, program: i32) -> String {
        self.presets
            .iter()
            .find(|p| p.bank == bank && p.program == program)
            .map(|p| p.name.clone())
            .unwrap_or_else(|| format!("Bank {bank} / {program}"))
    }
}

/// Keeps an output stream open on the current default device, reopening it
/// when it errors out or the default device changes.
fn output_loop(hub: Arc<Hub>, inner: Arc<Mutex<Inner>>) {
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
