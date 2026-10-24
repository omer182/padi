// Pad serial link (the Pico's USB CDC port, VID:PID 239A:CAFE). The firmware
// prints one line per event:
//   P <pad> <1|0>          pad pressed / released (pad 0..15)
//   S k=v k=v ...          full state snapshot whenever something changes
// The app writes "?\n" to ask for a fresh snapshot.
//
// Flashing needs the port free, so the link can be paused from the tray/UI.
use crate::hub::Hub;
use crate::midi::MidiLink;
use serde::Serialize;
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

const PAD_VID: u16 = 0x239A;
const PAD_PID: u16 = 0xCAFE;

#[derive(Clone, Serialize)]
pub struct PadEvent {
    pub pad: u8,
    pub down: bool,
}

pub struct DeviceLink {
    paused: AtomicBool,
    /// Write half of the open port, for host -> pad commands.
    writer: Mutex<Option<Box<dyn serialport::SerialPort>>>,
}

impl DeviceLink {
    pub fn start(hub: Arc<Hub>, midi: Arc<MidiLink>) -> Arc<DeviceLink> {
        let link = Arc::new(DeviceLink { paused: AtomicBool::new(false), writer: Mutex::new(None) });
        let l = link.clone();
        thread::Builder::new()
            .name("serial-watch".into())
            .spawn(move || watch(hub, midi, l))
            .expect("spawn serial watcher");
        link
    }

    pub fn set_paused(&self, hub: &Hub, paused: bool) {
        self.paused.store(paused, Ordering::Relaxed);
        hub.update(|s| s.serial_paused = paused);
        hub.log(if paused { "Released pad serial port (safe to flash)" } else { "Pad serial port resumed" });
    }

    /// Send one command line to the pad. Returns false if it isn't connected.
    pub fn send(&self, line: &str) -> bool {
        match self.writer.lock().unwrap().as_mut() {
            Some(w) => w.write_all(format!("{line}\n").as_bytes()).is_ok(),
            None => false,
        }
    }

    fn is_paused(&self) -> bool {
        self.paused.load(Ordering::Relaxed)
    }
}

fn find_port() -> Option<String> {
    serialport::available_ports().ok()?.into_iter().find_map(|p| match p.port_type {
        serialport::SerialPortType::UsbPort(u) if u.vid == PAD_VID && u.pid == PAD_PID => {
            Some(p.port_name)
        }
        _ => None,
    })
}

fn watch(hub: Arc<Hub>, midi: Arc<MidiLink>, link: Arc<DeviceLink>) {
    loop {
        if link.is_paused() {
            thread::sleep(Duration::from_millis(500));
            continue;
        }
        if let Some(name) = find_port() {
            if let Err(e) = session(&hub, &link, &name) {
                hub.log(format!("Pad serial {name}: {e}"));
            }
            *link.writer.lock().unwrap() = None;
            hub.update(|s| {
                s.pico_serial = false;
                s.device.clear();
            });
            // A dropped serial link usually means the pad reset; reopen MIDI too.
            midi.force_reconnect.store(true, Ordering::Relaxed);
        }
        thread::sleep(Duration::from_millis(1500));
    }
}

fn session(hub: &Hub, link: &DeviceLink, name: &str) -> Result<(), String> {
    // Never 1200 baud: that's arduino-pico's "reset into bootloader" signal.
    let mut port = serialport::new(name, 115_200)
        .timeout(Duration::from_millis(200))
        .open()
        .map_err(|e| e.to_string())?;
    // TinyUSB only sends while DTR is up (`if (Serial)` on the device).
    port.write_data_terminal_ready(true).map_err(|e| e.to_string())?;
    let _ = port.write_all(b"?\n");
    *link.writer.lock().unwrap() = port.try_clone().ok();
    hub.update(|s| s.pico_serial = true);
    hub.log(format!("Pad serial connected on {name}"));

    let mut reader = BufReader::new(port);
    let mut line = String::new();
    loop {
        if link.is_paused() {
            return Ok(()); // dropping the reader closes the port
        }
        match reader.read_line(&mut line) {
            Ok(0) => return Err("closed".into()),
            Ok(_) => {
                if line.ends_with('\n') {
                    handle_line(hub, line.trim());
                    line.clear();
                }
            }
            Err(e) if e.kind() == ErrorKind::TimedOut => {}
            Err(e) => return Err(e.to_string()),
        }
    }
}

fn handle_line(hub: &Hub, line: &str) {
    let mut parts = line.split_whitespace();
    match parts.next() {
        Some("P") => {
            let pad = parts.next().and_then(|v| v.parse::<u8>().ok());
            let down = parts.next() == Some("1");
            if let Some(pad) = pad.filter(|p| *p < 16) {
                hub.emit("pad", PadEvent { pad, down });
            }
        }
        Some("S") => {
            let state: BTreeMap<String, i32> = parts
                .filter_map(|kv| {
                    let (k, v) = kv.split_once('=')?;
                    Some((k.to_string(), v.parse().ok()?))
                })
                .collect();
            hub.update(|s| s.device = state);
        }
        _ => {}
    }
}
