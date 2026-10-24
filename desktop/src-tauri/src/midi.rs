// Pad MIDI link: finds "Macro MIDI Pad", plays everything it sends through the
// built-in engine, and mirrors the traffic to the UI. Reconnects on replug.
// (Windows MIDI is single-client: while the app holds the pad, a DAW can't.)
use crate::engine::{Cmd, Engine};
use crate::hub::{Hub, Sound};
use crate::sounds;
use midir::{Ignore, MidiInput, MidiInputConnection};
use serde::Serialize;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

pub const PAD_PORT_MATCH: &str = "Macro MIDI Pad";
const DRUM_CH: u8 = 9;

#[derive(Clone, Serialize)]
pub struct MidiEvent {
    pub kind: &'static str, // "on" | "off" | "cc" | "bend" | "pc"
    pub ch: u8,
    pub a: i32,
    pub b: i32,
}

pub struct MidiLink {
    /// Set by the serial link when the pad drops (e.g. reflash/reset) so the
    /// MIDI handle is reopened even if the port reappears between scans.
    pub force_reconnect: AtomicBool,
}

impl MidiLink {
    pub fn start(hub: Arc<Hub>, engine: Arc<Engine>) -> Arc<MidiLink> {
        let link = Arc::new(MidiLink { force_reconnect: AtomicBool::new(false) });
        let l = link.clone();
        thread::Builder::new()
            .name("midi-watch".into())
            .spawn(move || watch(hub, engine, l))
            .expect("spawn midi watcher");
        link
    }
}

/// Program the engine for a sound and tell the UI.
pub fn set_sound(hub: &Hub, engine: &Engine, ch: u8, bank: i32, program: i32) {
    engine.send(Cmd::Program { ch, bank, program });
    let name = engine.preset_name(bank, program);
    let sound = Some(Sound { bank, program, name });
    hub.update(|s| if ch == DRUM_CH { s.drum_kit = sound } else { s.sound = sound });
}

fn watch(hub: Arc<Hub>, engine: Arc<Engine>, link: Arc<MidiLink>) {
    let mut conn: Option<MidiInputConnection<()>> = None;
    loop {
        let present = pad_port_present();
        let forced = link.force_reconnect.swap(false, Ordering::Relaxed);
        if conn.is_some() && (!present || forced) {
            conn = None; // drop closes the handle
            hub.update(|s| s.pico_midi = false);
            hub.log("Pad MIDI disconnected");
        }
        if present && conn.is_none() {
            match connect(hub.clone(), engine.clone()) {
                Ok(c) => {
                    conn = Some(c);
                    hub.update(|s| s.pico_midi = true);
                    hub.log("Pad MIDI connected");
                }
                Err(e) => hub.log(format!("Pad MIDI connect failed: {e}")),
            }
        }
        thread::sleep(Duration::from_millis(1500));
    }
}

fn pad_port_present() -> bool {
    let Ok(input) = MidiInput::new("padi-scan") else { return false };
    input
        .ports()
        .iter()
        .any(|p| input.port_name(p).map(|n| n.contains(PAD_PORT_MATCH)).unwrap_or(false))
}

fn connect(hub: Arc<Hub>, engine: Arc<Engine>) -> Result<MidiInputConnection<()>, String> {
    let mut input = MidiInput::new("padi").map_err(|e| e.to_string())?;
    input.ignore(Ignore::All);
    let port = input
        .ports()
        .into_iter()
        .find(|p| input.port_name(p).map(|n| n.contains(PAD_PORT_MATCH)).unwrap_or(false))
        .ok_or("pad port vanished")?;

    let mut router = Router { hub, engine, pending_bank: [None; 16], warned: HashSet::new() };
    input
        .connect(&port, "padi-in", move |_, msg, _| router.handle(msg), ())
        .map_err(|e| e.to_string())
}

struct Router {
    hub: Arc<Hub>,
    engine: Arc<Engine>,
    /// Last Bank Select per channel = the pad's instrument category.
    pending_bank: [Option<u8>; 16],
    warned: HashSet<u8>,
}

impl Router {
    fn handle(&mut self, msg: &[u8]) {
        let Some(&status) = msg.first() else { return };
        if status >= 0xF0 {
            return;
        }
        let ch = status & 0x0F;
        let d1 = msg.get(1).copied().unwrap_or(0);
        let d2 = msg.get(2).copied().unwrap_or(0);
        match status & 0xF0 {
            0x90 if d2 > 0 => {
                self.engine.send(Cmd::Midi(status, d1, d2));
                self.emit("on", ch, d1 as i32, d2 as i32);
            }
            0x80 | 0x90 => {
                self.engine.send(Cmd::Midi(0x80 | ch, d1, 0));
                self.emit("off", ch, d1 as i32, 0);
            }
            0xB0 => self.control(ch, d1, d2),
            0xC0 => self.program(ch, d1),
            0xE0 => {
                self.engine.send(Cmd::Midi(status, d1, d2));
                self.emit("bend", ch, ((d2 as i32) << 7 | d1 as i32) - 8192, 0);
            }
            _ => {}
        }
    }

    fn control(&mut self, ch: u8, cc: u8, v: u8) {
        match cc {
            // Bank Select from the pad = its catalog category, not a SoundFont bank.
            0 if ch != DRUM_CH => {
                self.pending_bank[ch as usize] = Some(v);
                return;
            }
            74 => self.engine.send(Cmd::Filter(v)),
            1 | 7 | 10 | 11 | 64 | 91 | 93 | 120 | 121 | 123 => {
                self.engine.send(Cmd::Midi(0xB0 | ch, cc, v))
            }
            _ => {
                if self.warned.insert(cc) {
                    self.hub.log(format!("Knob effect CC{cc} isn't in the built-in engine yet"));
                }
            }
        }
        self.emit("cc", ch, cc as i32, v as i32);
    }

    fn program(&mut self, ch: u8, p: u8) {
        let target = match self.pending_bank[ch as usize].take() {
            Some(cat) => sounds::device_slot(cat, p).map(|gm| (cat, gm)),
            None => None,
        };
        let program = match target {
            Some((cat, gm)) => {
                self.hub.log(format!("Pad picked slot {}/{} -> GM {gm}", cat + 1, p + 1));
                gm
            }
            None => p,
        };
        set_sound(&self.hub, &self.engine, ch, 0, program as i32);
        self.emit("pc", ch, program as i32, 0);
    }

    fn emit(&self, kind: &'static str, ch: u8, a: i32, b: i32) {
        self.hub.emit("midi", MidiEvent { kind, ch, a, b });
    }
}
