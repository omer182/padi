// Sampler kits, record-to-pad, "My sounds", the looper and session recording.
//
// A kit is 16 pad slots saved as `kits/<id>/kit.json` + one WAV per take.
// In the pad's Sampler mode (firmware style 6) the pads send notes 36..51 on
// MIDI channel 16, which play the kit here. With record mode armed, holding a
// pad (on the device or on screen) records the mic onto it instead; record
// mode turns itself off after each take so the next press plays.
//
// Every kit edit is undoable (and redoable) until you switch kits.
use crate::device::DeviceLink;
use crate::engine::{Cmd, Engine, PlayMode, Slot, SoundParams, VoiceFx};
use crate::hub::{Hub, Sound};
use crate::mic::Mic;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

/// Firmware PlayStyle for Sampler mode.
pub const SAMPLER_STYLE: i32 = 6;
const PADS: usize = 16;
const UNDO_DEPTH: usize = 50;
/// Major-scale offsets used by "spread across pads" (pad 8 = original pitch).
const SPREAD: [f32; 16] = [-12., -10., -8., -7., -5., -3., -1., 0., 2., 4., 5., 7., 9., 11., 12., 14.];

static STUDIO: OnceLock<Arc<Studio>> = OnceLock::new();

pub fn get() -> Option<&'static Arc<Studio>> {
    STUDIO.get()
}

#[derive(Clone, Copy, PartialEq)]
pub enum Source {
    /// Note from the pad's Sampler mode (plays).
    Midi,
    /// Press reported over the serial link (records when armed).
    Serial,
    /// Clicked in the app (plays, or records when armed).
    Screen,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct PadSample {
    pub name: String,
    pub file: String,
    pub mode: PlayMode,
    pub gain: f32,
    pub pitch: f32,
    pub reverse: bool,
    #[serde(default)]
    pub echo: f32,
    #[serde(default)]
    pub fx: VoiceFx,
}

#[derive(Clone, Serialize, Deserialize)]
struct Kit {
    name: String,
    pads: Vec<Option<PadSample>>,
}

/// A stock sound saved with your own settings.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MySound {
    pub id: String,
    pub name: String,
    /// Name of the SoundFont it comes from (font indexes can change).
    pub font_name: String,
    pub bank: i32,
    pub program: i32,
    pub params: SoundParams,
}

#[derive(Serialize, Deserialize)]
struct Settings {
    kit: String,
    bpm: f32,
    bars: u32,
    metronome: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PadView {
    #[serde(flatten)]
    pub sample: PadSample,
    pub seconds: f32,
    pub peaks: Vec<f32>,
}

#[derive(Clone, Serialize)]
pub struct KitInfo {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StudioView {
    pub kit_id: String,
    pub kit_name: String,
    pub kits: Vec<KitInfo>,
    pub sounds: Vec<MySound>,
    pub pads: Vec<Option<PadView>>,
    pub active: bool,
    pub armed: bool,
    pub recording_pad: Option<u8>,
    pub can_undo: bool,
    pub can_redo: bool,
    pub bpm: f32,
    pub bars: u32,
    pub metronome: bool,
    pub layer_mutes: Vec<bool>,
    pub loop_redo: usize,
    pub session_file: Option<String>,
    pub exports_dir: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Transport {
    pos: f32,
    seconds: f32,
    playing: bool,
    recording: bool,
    countin_beats: u32,
    mic: f32,
}

type Audio = Option<(Arc<Vec<f32>>, u32)>;

/// Kit contents at one point in time (audio is shared, so this is cheap).
#[derive(Clone)]
struct Snapshot {
    pads: Vec<Option<PadSample>>,
    audio: Vec<Audio>,
}

struct State {
    kit_id: String,
    kit: Kit,
    audio: Vec<Audio>,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    /// (pad, when) of the last settings tweak, so a slider drag is one undo step.
    last_tweak: Option<(u8, Instant)>,
    active: bool,
    armed: bool,
    recording_pad: Option<u8>,
    bpm: f32,
    bars: u32,
    metronome: bool,
    session_file: Option<PathBuf>,
    sounds: Vec<MySound>,
}

impl State {
    fn snapshot(&self) -> Snapshot {
        Snapshot { pads: self.kit.pads.clone(), audio: self.audio.clone() }
    }

    /// Remember the current kit before an edit.
    fn checkpoint(&mut self) {
        let snap = self.snapshot();
        self.undo.push(snap);
        if self.undo.len() > UNDO_DEPTH {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.last_tweak = None;
    }
}

pub struct Studio {
    hub: Arc<Hub>,
    engine: Arc<Engine>,
    device: Arc<DeviceLink>,
    mic: Mic,
    kits_dir: PathBuf,
    settings_file: PathBuf,
    sounds_file: PathBuf,
    exports_dir: PathBuf,
    state: Mutex<State>,
}

impl Studio {
    pub fn start(
        hub: Arc<Hub>,
        engine: Arc<Engine>,
        device: Arc<DeviceLink>,
        data_dir: &Path,
        exports_dir: PathBuf,
    ) -> Arc<Studio> {
        let kits_dir = data_dir.join("kits");
        let settings_file = data_dir.join("settings.json");
        let sounds_file = data_dir.join("sounds.json");
        let _ = fs::create_dir_all(&kits_dir);
        let _ = fs::create_dir_all(&exports_dir);
        let settings: Option<Settings> =
            fs::read_to_string(&settings_file).ok().and_then(|s| serde_json::from_str(&s).ok());
        let sounds: Vec<MySound> =
            fs::read_to_string(&sounds_file).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();

        let studio = Arc::new(Studio {
            mic: Mic::start(hub.clone()),
            hub,
            engine,
            device,
            kits_dir,
            settings_file,
            sounds_file,
            exports_dir,
            state: Mutex::new(State {
                kit_id: String::new(),
                kit: empty_kit("My Kit"),
                audio: vec![None; PADS],
                undo: Vec::new(),
                redo: Vec::new(),
                last_tweak: None,
                active: false,
                armed: false,
                recording_pad: None,
                bpm: settings.as_ref().map_or(100.0, |s| s.bpm),
                bars: settings.as_ref().map_or(2, |s| s.bars),
                metronome: settings.as_ref().map_or(true, |s| s.metronome),
                session_file: None,
                sounds,
            }),
        });
        {
            let st = studio.state.lock().unwrap();
            studio.engine.send(Cmd::LooperTempo { bpm: st.bpm, bars: st.bars });
            studio.engine.send(Cmd::Metronome(st.metronome));
        }
        let wanted = settings.map(|s| s.kit).filter(|id| studio.kits_dir.join(id).join("kit.json").exists());
        match wanted.or_else(|| studio.list_kits().first().map(|k| k.id.clone())) {
            Some(id) => {
                let _ = studio.load_kit(&id);
            }
            None => {
                let _ = studio.new_kit("My Kit");
            }
        }
        let _ = STUDIO.set(studio.clone());
        let s = studio.clone();
        thread::Builder::new().name("transport".into()).spawn(move || s.ticker()).expect("spawn ticker");
        studio
    }

    // ---------------------------------------------------------------- view

    pub fn view(&self) -> StudioView {
        let st = self.state.lock().unwrap();
        let pads = st
            .kit
            .pads
            .iter()
            .zip(&st.audio)
            .map(|(p, a)| {
                let (p, (data, sr)) = (p.as_ref()?, a.as_ref()?);
                Some(PadView { sample: p.clone(), seconds: data.len() as f32 / *sr as f32, peaks: peaks(data, 40) })
            })
            .collect();
        StudioView {
            kit_id: st.kit_id.clone(),
            kit_name: st.kit.name.clone(),
            kits: self.list_kits(),
            sounds: st.sounds.clone(),
            pads,
            active: st.active,
            armed: st.armed,
            recording_pad: st.recording_pad,
            can_undo: !st.undo.is_empty(),
            can_redo: !st.redo.is_empty(),
            bpm: st.bpm,
            bars: st.bars,
            metronome: st.metronome,
            layer_mutes: self.engine.looper.mutes.lock().unwrap().clone(),
            loop_redo: self.engine.looper.redo.load(Ordering::Relaxed),
            session_file: st.session_file.as_ref().map(|p| p.display().to_string()),
            exports_dir: self.exports_dir.display().to_string(),
        }
    }

    fn publish(&self) {
        self.hub.emit("studio", self.view());
    }

    fn ticker(&self) {
        let mut seen = (usize::MAX, usize::MAX);
        loop {
            thread::sleep(Duration::from_millis(40));
            let lp = &self.engine.looper;
            let now = (lp.version.load(Ordering::Relaxed), lp.redo.load(Ordering::Relaxed));
            if now != seen {
                seen = now;
                self.publish();
            }
            let len = lp.len.load(Ordering::Relaxed);
            let (armed, bpm) = {
                let st = self.state.lock().unwrap();
                (st.armed, st.bpm)
            };
            let sr = self.engine.sample_rate().max(1) as f32;
            let beat = sr * 60.0 / bpm;
            let countin = lp.countin.load(Ordering::Relaxed);
            let has_layers = !lp.mutes.lock().unwrap().is_empty();
            self.hub.emit(
                "transport",
                Transport {
                    pos: if len > 0 { lp.pos.load(Ordering::Relaxed) as f32 / len as f32 } else { 0.0 },
                    seconds: if has_layers || lp.recording.load(Ordering::Relaxed) { len as f32 / sr } else { 0.0 },
                    playing: lp.playing.load(Ordering::Relaxed),
                    recording: lp.recording.load(Ordering::Relaxed),
                    countin_beats: if countin > 0 { (countin as f32 / beat).ceil() as u32 } else { 0 },
                    mic: if armed { self.mic.take_peak() } else { 0.0 },
                },
            );
        }
    }

    // ---------------------------------------------------------------- pads

    /// Make the pads play this kit: puts the pad into Sampler mode.
    pub fn activate(&self, on: bool) {
        self.state.lock().unwrap().active = on;
        if on {
            self.device.send(&format!("st {SAMPLER_STYLE}"));
        }
        self.publish();
    }

    pub fn arm(&self, on: bool) {
        {
            let mut st = self.state.lock().unwrap();
            st.armed = on;
            if !on {
                st.recording_pad = None;
            }
        }
        self.mic.arm(on);
        if on {
            self.activate(true);
        } else {
            self.publish();
        }
    }

    pub fn pad_input(&self, pad: u8, down: bool, velocity: u8, src: Source) {
        if pad as usize >= PADS {
            return;
        }
        let armed = self.state.lock().unwrap().armed;
        if armed {
            if src == Source::Midi {
                return; // the serial press records; don't also play
            }
            if down {
                self.record_begin(pad);
            } else {
                self.record_end(pad);
            }
            return;
        }
        if src == Source::Serial {
            return; // in play mode the MIDI note does the triggering
        }
        self.engine.send(if down { Cmd::PadOn(pad, velocity) } else { Cmd::PadOff(pad) });
    }

    /// Arm and record the next press onto exactly this pad (from a menu).
    pub fn record_into(&self, pad: u8) {
        self.arm(true);
        self.hub.log(format!("Hold pad {} and make a noise", pad + 1));
    }

    fn record_begin(&self, pad: u8) {
        let mut st = self.state.lock().unwrap();
        if st.recording_pad.is_some() {
            return;
        }
        st.recording_pad = Some(pad);
        drop(st);
        self.mic.begin();
        self.publish();
    }

    fn record_end(&self, pad: u8) {
        {
            let mut st = self.state.lock().unwrap();
            if st.recording_pad != Some(pad) {
                return;
            }
            st.recording_pad = None;
        }
        let (take, sr) = self.mic.end();
        match clean_take(take, sr) {
            Some(audio) => {
                let n = self.state.lock().unwrap().kit.pads.iter().flatten().count() + 1;
                let file = format!("pad{:02}-{}.wav", pad + 1, chrono::Local::now().format("%Y%m%d-%H%M%S"));
                match write_wav_mono(&self.kit_dir().join(&file), &audio, sr) {
                    Ok(()) => {
                        let sample = PadSample {
                            name: format!("Take {n}"),
                            file,
                            mode: PlayMode::Oneshot,
                            gain: 1.0,
                            pitch: 0.0,
                            reverse: false,
                            echo: 0.0,
                            fx: VoiceFx::None,
                        };
                        self.hub.log(format!("Recorded {:.1}s onto pad {}", audio.len() as f32 / sr as f32, pad + 1));
                        self.state.lock().unwrap().checkpoint();
                        self.put_pad(pad, Some((sample, Arc::new(audio), sr)));
                        // Done: the next press should play it, not record over it.
                        self.arm(false);
                    }
                    Err(e) => self.hub.log(format!("Couldn't save take: {e}")),
                }
            }
            None => self.hub.log("Nothing heard. Hold the pad and make some noise"),
        }
        self.publish();
    }

    fn put_pad(&self, pad: u8, value: Option<(PadSample, Arc<Vec<f32>>, u32)>) {
        let mut st = self.state.lock().unwrap();
        let i = pad as usize;
        match value {
            Some((sample, audio, sr)) => {
                self.engine.send(Cmd::SetSlot(pad, Some(make_slot(&sample, &audio, sr))));
                st.kit.pads[i] = Some(sample);
                st.audio[i] = Some((audio, sr));
            }
            None => {
                self.engine.send(Cmd::SetSlot(pad, None));
                st.kit.pads[i] = None;
                st.audio[i] = None;
            }
        }
        drop(st);
        self.save_kit();
    }

    pub fn update_pad(&self, pad: u8, sample: PadSample) {
        let mut st = self.state.lock().unwrap();
        let Some((audio, sr)) = st.audio.get(pad as usize).cloned().flatten() else { return };
        // A drag on one pad's sliders is one undo step.
        let same_drag = matches!(st.last_tweak, Some((p, t)) if p == pad && t.elapsed() < Duration::from_millis(1500));
        if !same_drag {
            st.checkpoint();
        }
        st.last_tweak = Some((pad, Instant::now()));
        // The file is fixed by the take; only the settings change here.
        let file = st.kit.pads[pad as usize].as_ref().map(|p| p.file.clone()).unwrap_or(sample.file.clone());
        drop(st);
        self.put_pad(pad, Some((PadSample { file, ..sample }, audio, sr)));
        self.publish();
    }

    pub fn clear_pad(&self, pad: u8) {
        self.state.lock().unwrap().checkpoint();
        self.put_pad(pad, None);
        self.publish();
    }

    /// Copy one pad onto another (its own settings, same recording).
    pub fn copy_pad(&self, from: u8, to: u8) {
        let src = {
            let st = self.state.lock().unwrap();
            st.kit.pads[from as usize].clone().zip(st.audio[from as usize].clone())
        };
        let Some((sample, (audio, sr))) = src else { return };
        self.state.lock().unwrap().checkpoint();
        self.put_pad(to, Some((sample, audio, sr)));
        self.publish();
    }

    /// Copy one pad's sample across all 16 pads, pitched up a major scale.
    pub fn spread(&self, pad: u8) {
        let src = {
            let st = self.state.lock().unwrap();
            st.kit.pads[pad as usize].clone().zip(st.audio[pad as usize].clone())
        };
        let Some((sample, (audio, sr))) = src else { return };
        self.state.lock().unwrap().checkpoint();
        for (i, semis) in SPREAD.iter().enumerate() {
            let s = PadSample { name: format!("{} {}", sample.name, i + 1), pitch: sample.pitch + semis, ..sample.clone() };
            self.put_pad(i as u8, Some((s, audio.clone(), sr)));
        }
        self.publish();
    }

    pub fn undo(&self, redo: bool) {
        let snap = {
            let mut st = self.state.lock().unwrap();
            let Some(snap) = (if redo { st.redo.pop() } else { st.undo.pop() }) else { return };
            let current = st.snapshot();
            if redo { st.undo.push(current) } else { st.redo.push(current) }
            st.last_tweak = None;
            snap
        };
        self.restore(snap);
        self.save_kit();
        self.publish();
    }

    fn restore(&self, snap: Snapshot) {
        let mut st = self.state.lock().unwrap();
        for i in 0..PADS {
            let slot = snap.pads[i].as_ref().zip(snap.audio[i].as_ref()).map(|(p, (a, sr))| make_slot(p, a, *sr));
            self.engine.send(Cmd::SetSlot(i as u8, slot));
        }
        st.kit.pads = snap.pads;
        st.audio = snap.audio;
    }

    // ---------------------------------------------------------------- kits

    fn kit_dir(&self) -> PathBuf {
        self.kits_dir.join(&self.state.lock().unwrap().kit_id)
    }

    pub fn list_kits(&self) -> Vec<KitInfo> {
        let mut kits: Vec<KitInfo> = fs::read_dir(&self.kits_dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                let id = e.file_name().to_string_lossy().to_string();
                let kit: Kit = serde_json::from_str(&fs::read_to_string(e.path().join("kit.json")).ok()?).ok()?;
                Some(KitInfo { id, name: kit.name })
            })
            .collect();
        kits.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        kits
    }

    fn free_id(&self, name: &str) -> String {
        let base = slug(name);
        let mut id = base.clone();
        let mut n = 2;
        while self.kits_dir.join(&id).exists() {
            id = format!("{base}-{n}");
            n += 1;
        }
        id
    }

    pub fn new_kit(&self, name: &str) -> Result<(), String> {
        let id = self.free_id(name);
        fs::create_dir_all(self.kits_dir.join(&id)).map_err(|e| e.to_string())?;
        {
            let mut st = self.state.lock().unwrap();
            st.kit_id = id;
            st.kit = empty_kit(name);
            st.audio = vec![None; PADS];
            st.undo.clear();
            st.redo.clear();
        }
        for pad in 0..PADS as u8 {
            self.engine.send(Cmd::SetSlot(pad, None));
        }
        self.save_kit();
        self.save_settings();
        self.publish();
        Ok(())
    }

    pub fn load_kit(&self, id: &str) -> Result<(), String> {
        let dir = self.kits_dir.join(id);
        let mut kit: Kit = serde_json::from_str(&fs::read_to_string(dir.join("kit.json")).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        kit.pads.resize(PADS, None);
        let mut audio = vec![None; PADS];
        for (i, pad) in kit.pads.iter_mut().enumerate() {
            let Some(p) = pad.as_ref() else { continue };
            match read_wav_mono(&dir.join(&p.file)) {
                Ok((data, sr)) => audio[i] = Some((Arc::new(data), sr)),
                Err(e) => {
                    self.hub.log(format!("Pad {}: can't read {}: {e}", i + 1, p.file));
                    *pad = None;
                }
            }
        }
        {
            let mut st = self.state.lock().unwrap();
            st.kit_id = id.to_string();
            st.undo.clear();
            st.redo.clear();
            st.kit.name = kit.name.clone();
        }
        self.restore(Snapshot { pads: kit.pads, audio });
        self.save_settings();
        self.publish();
        Ok(())
    }

    pub fn rename_kit(&self, id: &str, name: &str) {
        let name = name.trim().to_string();
        let current = self.state.lock().unwrap().kit_id == id;
        if current {
            self.state.lock().unwrap().kit.name = name;
            self.save_kit();
        } else {
            let path = self.kits_dir.join(id).join("kit.json");
            if let Some(mut kit) = fs::read_to_string(&path).ok().and_then(|s| serde_json::from_str::<Kit>(&s).ok()) {
                kit.name = name;
                if let Ok(json) = serde_json::to_string_pretty(&kit) {
                    let _ = fs::write(path, json);
                }
            }
        }
        self.publish();
    }

    /// Copy a kit (all its takes) under a new name and switch to the copy.
    pub fn duplicate_kit(&self, id: &str) -> Result<(), String> {
        let src = self.kits_dir.join(id);
        let mut kit: Kit = serde_json::from_str(&fs::read_to_string(src.join("kit.json")).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        kit.name = format!("{} copy", kit.name);
        let new_id = self.free_id(&kit.name);
        let dst = self.kits_dir.join(&new_id);
        fs::create_dir_all(&dst).map_err(|e| e.to_string())?;
        for e in fs::read_dir(&src).map_err(|e| e.to_string())?.flatten() {
            if e.path().extension().is_some_and(|x| x == "wav") {
                fs::copy(e.path(), dst.join(e.file_name())).map_err(|e| e.to_string())?;
            }
        }
        fs::write(dst.join("kit.json"), serde_json::to_string_pretty(&kit).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        self.load_kit(&new_id)
    }

    /// Delete a kit folder (the UI confirms first). Switches away if it's open.
    pub fn delete_kit(&self, id: &str) -> Result<(), String> {
        if id.is_empty() || id.contains(['/', '\\']) || id.contains("..") {
            return Err("bad kit id".into());
        }
        fs::remove_dir_all(self.kits_dir.join(id)).map_err(|e| e.to_string())?;
        if self.state.lock().unwrap().kit_id == id {
            match self.list_kits().first() {
                Some(k) => self.load_kit(&k.id.clone())?,
                None => self.new_kit("My Kit")?,
            }
        }
        self.publish();
        Ok(())
    }

    fn save_kit(&self) {
        let (dir, json) = {
            let st = self.state.lock().unwrap();
            (self.kits_dir.join(&st.kit_id), serde_json::to_string_pretty(&st.kit))
        };
        if let Ok(json) = json {
            if let Err(e) = fs::write(dir.join("kit.json"), json) {
                self.hub.log(format!("Couldn't save kit: {e}"));
            }
        }
    }

    fn save_settings(&self) {
        let st = self.state.lock().unwrap();
        let s = Settings { kit: st.kit_id.clone(), bpm: st.bpm, bars: st.bars, metronome: st.metronome };
        if let Ok(json) = serde_json::to_string_pretty(&s) {
            let _ = fs::write(&self.settings_file, json);
        }
    }

    // ---------------------------------------------------------------- my sounds

    /// Save the current stock (or own) sound with its settings as one of yours.
    /// With `id`, overwrites that sound; otherwise makes a new one.
    pub fn save_sound(&self, id: Option<String>, name: &str, sound: &Sound) -> String {
        let font_name = self.engine.font_names().get(sound.font).cloned().unwrap_or_default();
        let mut st = self.state.lock().unwrap();
        let id = id.unwrap_or_else(|| format!("s{}", chrono::Local::now().format("%Y%m%d%H%M%S%3f")));
        let entry = MySound {
            id: id.clone(),
            name: name.trim().to_string(),
            font_name,
            bank: sound.bank,
            program: sound.program,
            params: sound.params,
        };
        match st.sounds.iter_mut().find(|s| s.id == id) {
            Some(s) => *s = entry,
            None => st.sounds.push(entry),
        }
        drop(st);
        self.save_sounds();
        self.publish();
        id
    }

    pub fn find_sound(&self, id: &str) -> Option<MySound> {
        self.state.lock().unwrap().sounds.iter().find(|s| s.id == id).cloned()
    }

    pub fn rename_sound(&self, id: &str, name: &str) {
        if let Some(s) = self.state.lock().unwrap().sounds.iter_mut().find(|s| s.id == id) {
            s.name = name.trim().to_string();
        }
        self.save_sounds();
        self.publish();
    }

    pub fn delete_sound(&self, id: &str) {
        self.state.lock().unwrap().sounds.retain(|s| s.id != id);
        self.save_sounds();
        self.publish();
    }

    fn save_sounds(&self) {
        let json = serde_json::to_string_pretty(&self.state.lock().unwrap().sounds);
        if let Ok(json) = json {
            let _ = fs::write(&self.sounds_file, json);
        }
    }

    // ---------------------------------------------------------------- looper

    pub fn looper_record(&self) {
        let sr = self.engine.sample_rate() as f32;
        let has_layers = !self.engine.looper.mutes.lock().unwrap().is_empty();
        let len = match self.engine.looper.len.load(Ordering::Relaxed) {
            n if n > 0 && has_layers => n,
            _ => {
                let st = self.state.lock().unwrap();
                (sr * 60.0 / st.bpm * 4.0 * st.bars as f32) as usize
            }
        };
        self.engine.send(Cmd::LooperRecord(vec![0.0; len * 2]));
    }

    pub fn looper_cmd(&self, cmd: &str, layer: Option<usize>) {
        match (cmd, layer) {
            ("stop", _) => self.engine.send(Cmd::LooperStop),
            ("undo", _) => self.engine.send(Cmd::LooperUndo),
            ("redo", _) => self.engine.send(Cmd::LooperRedo),
            ("clear", _) => self.engine.send(Cmd::LooperClear),
            ("remove", Some(i)) => self.engine.send(Cmd::LooperRemove(i)),
            _ => {}
        }
    }

    pub fn looper_mute(&self, layer: usize, muted: bool) {
        self.engine.send(Cmd::LooperMute(layer, muted));
    }

    /// Mute every layer except this one (or unmute all if it's already solo).
    pub fn looper_solo(&self, layer: usize) {
        let mutes = self.engine.looper.mutes.lock().unwrap().clone();
        let already = mutes.iter().enumerate().all(|(i, m)| *m == (i != layer));
        for i in 0..mutes.len() {
            self.engine.send(Cmd::LooperMute(i, !already && i != layer));
        }
    }

    pub fn set_tempo(&self, bpm: f32, bars: u32, metronome: bool) {
        {
            let mut st = self.state.lock().unwrap();
            st.bpm = bpm.clamp(40.0, 220.0);
            st.bars = bars.clamp(1, 16);
            st.metronome = metronome;
            self.engine.send(Cmd::LooperTempo { bpm: st.bpm, bars: st.bars });
            self.engine.send(Cmd::Metronome(metronome));
        }
        self.save_settings();
        self.publish();
    }

    // ---------------------------------------------------------------- saving

    /// Start/stop recording everything you hear (minus the click) to a WAV.
    pub fn toggle_session(&self) -> Result<Option<String>, String> {
        let mut st = self.state.lock().unwrap();
        if let Some(path) = st.session_file.take() {
            self.engine.send(Cmd::Tap(None)); // writer finishes when the tap drops
            drop(st);
            self.publish();
            return Ok(Some(path.display().to_string()));
        }
        fs::create_dir_all(&self.exports_dir).map_err(|e| e.to_string())?;
        let path = self.exports_dir.join(format!("PADI session {}.wav", chrono::Local::now().format("%Y-%m-%d %H-%M-%S")));
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: self.engine.sample_rate(),
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&path, spec).map_err(|e| e.to_string())?;
        let (tx, rx) = channel::<Vec<f32>>();
        let hub = self.hub.clone();
        let shown = path.display().to_string();
        thread::spawn(move || {
            for chunk in rx {
                for s in chunk {
                    let _ = writer.write_sample((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16);
                }
            }
            match writer.finalize() {
                Ok(()) => hub.log(format!("Saved session: {shown}")),
                Err(e) => hub.log(format!("Session save failed: {e}")),
            }
        });
        self.engine.send(Cmd::Tap(Some(tx)));
        st.session_file = Some(path);
        drop(st);
        self.hub.log("Recording session…");
        self.publish();
        Ok(None)
    }

    /// Write one cycle of the loop (unmuted layers) to a WAV.
    pub fn export_loop(&self) -> Result<String, String> {
        let (tx, rx) = channel();
        self.engine.send(Cmd::Layers(tx));
        let (len, layers) = rx.recv_timeout(Duration::from_secs(2)).map_err(|_| "audio engine didn't answer")?;
        if len == 0 || layers.is_empty() {
            return Err("Record a loop first (muted layers are left out)".into());
        }
        let mut mix = vec![0f32; len * 2];
        for layer in &layers {
            for (m, s) in mix.iter_mut().zip(layer.iter()) {
                *m += s;
            }
        }
        let peak = mix.iter().fold(0f32, |p, s| p.max(s.abs()));
        let gain = if peak > 0.99 { 0.99 / peak } else { 1.0 };
        fs::create_dir_all(&self.exports_dir).map_err(|e| e.to_string())?;
        let path = self.exports_dir.join(format!("PADI loop {}.wav", chrono::Local::now().format("%Y-%m-%d %H-%M-%S")));
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: self.engine.sample_rate(),
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(&path, spec).map_err(|e| e.to_string())?;
        for s in mix {
            w.write_sample(((s * gain).clamp(-1.0, 1.0) * i16::MAX as f32) as i16).map_err(|e| e.to_string())?;
        }
        w.finalize().map_err(|e| e.to_string())?;
        let shown = path.display().to_string();
        self.hub.log(format!("Exported loop: {shown}"));
        Ok(shown)
    }
}

fn empty_kit(name: &str) -> Kit {
    Kit { name: name.trim().to_string(), pads: vec![None; PADS] }
}

fn slug(name: &str) -> String {
    let s: String = name
        .trim()
        .chars()
        .map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if s.is_empty() { "kit".into() } else { s }
}

fn make_slot(p: &PadSample, audio: &Arc<Vec<f32>>, sr: u32) -> Arc<Slot> {
    let data = if p.reverse { Arc::new(audio.iter().rev().copied().collect()) } else { audio.clone() };
    Arc::new(Slot { data, sample_rate: sr, mode: p.mode, gain: p.gain, pitch: p.pitch, echo: p.echo, fx: p.fx })
}

/// Trim silence, normalize to about -1 dB and fade the edges. None = no sound.
fn clean_take(mut x: Vec<f32>, sr: u32) -> Option<Vec<f32>> {
    let peak = x.iter().fold(0f32, |p, s| p.max(s.abs()));
    if peak < 0.01 {
        return None;
    }
    let thr = (peak * 0.06).max(0.008);
    let first = x.iter().position(|s| s.abs() > thr)?;
    let last = x.iter().rposition(|s| s.abs() > thr)?;
    let start = first.saturating_sub(sr as usize / 100);
    let end = (last + sr as usize * 8 / 100).min(x.len());
    if end - start < sr as usize / 20 {
        return None;
    }
    x.truncate(end);
    x.drain(..start);
    let gain = 0.89 / peak;
    let fade_in = (sr as usize / 200).min(x.len() / 2);
    let fade_out = (sr as usize / 50).min(x.len() / 2);
    let n = x.len();
    for (i, s) in x.iter_mut().enumerate() {
        let mut g = gain;
        if i < fade_in {
            g *= i as f32 / fade_in as f32;
        }
        if i >= n - fade_out {
            g *= (n - i) as f32 / fade_out as f32;
        }
        *s *= g;
    }
    Some(x)
}

fn peaks(data: &[f32], bins: usize) -> Vec<f32> {
    let chunk = (data.len() / bins).max(1);
    data.chunks(chunk).take(bins).map(|c| c.iter().fold(0f32, |p, s| p.max(s.abs()))).collect()
}

fn write_wav_mono(path: &Path, data: &[f32], sr: u32) -> Result<(), String> {
    let spec = hound::WavSpec { channels: 1, sample_rate: sr, bits_per_sample: 32, sample_format: hound::SampleFormat::Float };
    let mut w = hound::WavWriter::create(path, spec).map_err(|e| e.to_string())?;
    for &s in data {
        w.write_sample(s).map_err(|e| e.to_string())?;
    }
    w.finalize().map_err(|e| e.to_string())
}

fn read_wav_mono(path: &Path) -> Result<(Vec<f32>, u32), String> {
    let mut r = hound::WavReader::open(path).map_err(|e| e.to_string())?;
    let spec = r.spec();
    let ch = spec.channels.max(1) as usize;
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => r.samples::<f32>().filter_map(Result::ok).collect(),
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1i64 << (spec.bits_per_sample - 1)) as f32;
            r.samples::<i32>().filter_map(Result::ok).map(|s| s as f32 * scale).collect()
        }
    };
    let mono = samples.chunks(ch).map(|f| f.iter().sum::<f32>() / ch as f32).collect();
    Ok((mono, spec.sample_rate))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_take_trims_and_normalizes() {
        let sr = 48000;
        let mut x = vec![0.001f32; sr]; // 1s of near-silence
        x.extend((0..sr / 4).map(|n| (n as f32 * 0.1).sin() * 0.2)); // 0.25s of sound
        x.extend(vec![0.0; sr]);
        let y = clean_take(x, sr as u32).expect("has sound");
        let secs = y.len() as f32 / sr as f32;
        assert!((0.25..0.4).contains(&secs), "trimmed to the sound: {secs}s");
        let peak = y.iter().fold(0f32, |p, s| p.max(s.abs()));
        assert!((0.85..0.9).contains(&peak), "normalized: {peak}");
        assert!(clean_take(vec![0.002; sr], sr as u32).is_none(), "silence is no take");
    }

    #[test]
    fn wav_round_trip_and_slug() {
        let dir = std::env::temp_dir().join("padi-test");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.wav");
        let data: Vec<f32> = (0..1000).map(|n| n as f32 / 1000.0).collect();
        write_wav_mono(&path, &data, 44100).unwrap();
        let (back, sr) = read_wav_mono(&path).unwrap();
        assert_eq!((back.len(), sr), (1000, 44100));
        assert!((back[500] - 0.5).abs() < 1e-6);
        assert_eq!(slug("  Funny Noises #1 "), "funny-noises-1");
        assert_eq!(slug("!!!"), "kit");
    }

    #[test]
    fn old_kit_files_still_load() {
        // Kits saved before Echo/FX existed have no such fields.
        let json = r#"{"name":"K","pads":[{"name":"a","file":"a.wav","mode":"loop","gain":1.0,"pitch":0.0,"reverse":false},null]}"#;
        let kit: Kit = serde_json::from_str(json).unwrap();
        let p = kit.pads[0].as_ref().unwrap();
        assert_eq!((p.mode, p.echo, p.fx), (PlayMode::Loop, 0.0, VoiceFx::None));
    }
}
