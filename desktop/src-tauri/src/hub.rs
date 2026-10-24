// Shared app state + event fan-out to the UI. Every subsystem (engine, MIDI
// link, serial link) writes here; the UI gets a full `status` snapshot on each
// change plus lightweight `midi` / `pad` / `log` events.
use serde::Serialize;
use std::collections::{BTreeMap, VecDeque};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter};

#[derive(Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct EngineStatus {
    pub ok: bool,
    pub output: String,
    pub sample_rate: u32,
    pub error: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Sound {
    pub bank: i32,
    pub program: i32,
    pub name: String,
}

#[derive(Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub engine: EngineStatus,
    pub pico_midi: bool,
    pub pico_serial: bool,
    pub serial_paused: bool,
    pub sound: Option<Sound>,
    pub drum_kit: Option<Sound>,
    pub volume: f32,
    /// Latest firmware state line (`S k=v ...`), empty until the pad reports.
    pub device: BTreeMap<String, i32>,
}

const LOG_KEEP: usize = 200;

pub struct Hub {
    app: AppHandle,
    status: Mutex<Status>,
    log: Mutex<VecDeque<String>>,
}

impl Hub {
    pub fn new(app: AppHandle) -> Self {
        Hub {
            app,
            status: Mutex::new(Status { volume: 0.8, ..Default::default() }),
            log: Mutex::new(VecDeque::new()),
        }
    }

    pub fn status(&self) -> Status {
        self.status.lock().unwrap().clone()
    }

    /// Mutate the status and push the new snapshot to the UI.
    pub fn update(&self, f: impl FnOnce(&mut Status)) {
        let snap = {
            let mut s = self.status.lock().unwrap();
            f(&mut s);
            s.clone()
        };
        let _ = self.app.emit("status", snap);
    }

    pub fn emit<S: Serialize + Clone>(&self, event: &str, payload: S) {
        let _ = self.app.emit(event, payload);
    }

    pub fn log(&self, msg: impl Into<String>) {
        let msg = msg.into();
        eprintln!("[padi] {msg}");
        {
            let mut l = self.log.lock().unwrap();
            l.push_back(msg.clone());
            while l.len() > LOG_KEEP {
                l.pop_front();
            }
        }
        let _ = self.app.emit("log", msg);
    }

    pub fn log_lines(&self) -> Vec<String> {
        self.log.lock().unwrap().iter().cloned().collect()
    }
}
