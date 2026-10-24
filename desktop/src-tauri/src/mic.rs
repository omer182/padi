// Microphone capture for recording samples onto pads. The input stream is
// only open while record mode is armed (so the OS mic indicator means
// "PADI is listening"), and audio is kept only between start() and stop().
use crate::hub::Hub;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample, StreamConfig};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

/// Longest take kept per pad.
const MAX_SECONDS: usize = 20;

enum MicCmd {
    Arm(bool),
}

#[derive(Default)]
struct Capture {
    recording: AtomicBool,
    buf: Mutex<Vec<f32>>,
    sample_rate: AtomicU32,
    /// Peak level since the UI last read it (f32 bits).
    peak: AtomicU32,
}

pub struct Mic {
    tx: Sender<MicCmd>,
    cap: Arc<Capture>,
}

impl Mic {
    pub fn start(hub: Arc<Hub>) -> Mic {
        let (tx, rx) = channel::<MicCmd>();
        let cap = Arc::new(Capture::default());
        let c = cap.clone();
        thread::Builder::new()
            .name("mic".into())
            .spawn(move || {
                let mut stream: Option<cpal::Stream> = None;
                for cmd in rx {
                    match cmd {
                        MicCmd::Arm(true) if stream.is_none() => match open(c.clone()) {
                            Ok((s, name)) => {
                                hub.log(format!("Mic on: {name}"));
                                stream = Some(s);
                            }
                            Err(e) => hub.log(format!("Mic: {e}")),
                        },
                        MicCmd::Arm(false) if stream.is_some() => {
                            stream = None;
                            c.recording.store(false, Ordering::Relaxed);
                            hub.log("Mic off");
                        }
                        _ => {}
                    }
                }
            })
            .expect("spawn mic thread");
        Mic { tx, cap }
    }

    pub fn arm(&self, on: bool) {
        let _ = self.tx.send(MicCmd::Arm(on));
    }

    pub fn begin(&self) {
        self.cap.buf.lock().unwrap().clear();
        self.cap.recording.store(true, Ordering::Relaxed);
    }

    /// Stop recording; returns the mono take and its sample rate.
    pub fn end(&self) -> (Vec<f32>, u32) {
        self.cap.recording.store(false, Ordering::Relaxed);
        let take = std::mem::take(&mut *self.cap.buf.lock().unwrap());
        (take, self.cap.sample_rate.load(Ordering::Relaxed))
    }

    /// Peak input level (0..1) since the last call.
    pub fn take_peak(&self) -> f32 {
        f32::from_bits(self.cap.peak.swap(0, Ordering::Relaxed))
    }
}

fn open(cap: Arc<Capture>) -> Result<(cpal::Stream, String), String> {
    let device = cpal::default_host().default_input_device().ok_or("no microphone found")?;
    let name = device.description().map(|d| d.name().to_string()).unwrap_or_else(|_| "Microphone".into());
    let supported = device.default_input_config().map_err(|e| e.to_string())?;
    let format = supported.sample_format();
    let config: StreamConfig = supported.into();
    cap.sample_rate.store(config.sample_rate, Ordering::Relaxed);
    let stream = match format {
        SampleFormat::F32 => build::<f32>(&device, config, cap),
        SampleFormat::I16 => build::<i16>(&device, config, cap),
        SampleFormat::I32 => build::<i32>(&device, config, cap),
        SampleFormat::U16 => build::<u16>(&device, config, cap),
        other => return Err(format!("unsupported mic format {other}")),
    }?;
    stream.play().map_err(|e| e.to_string())?;
    Ok((stream, name))
}

fn build<T: SizedSample>(device: &cpal::Device, config: StreamConfig, cap: Arc<Capture>) -> Result<cpal::Stream, String>
where
    f32: FromSample<T>,
{
    let channels = config.channels as usize;
    let max = config.sample_rate as usize * MAX_SECONDS;
    device
        .build_input_stream(
            config,
            move |data: &[T], _: &cpal::InputCallbackInfo| {
                let mut peak = f32::from_bits(cap.peak.load(Ordering::Relaxed));
                let recording = cap.recording.load(Ordering::Relaxed);
                let mut buf = if recording { cap.buf.try_lock().ok() } else { None };
                for frame in data.chunks(channels) {
                    let m = frame.iter().map(|s| s.to_sample::<f32>()).sum::<f32>() / channels as f32;
                    peak = peak.max(m.abs());
                    if let Some(b) = buf.as_mut() {
                        if b.len() < max {
                            b.push(m);
                        }
                    }
                }
                cap.peak.store(peak.to_bits(), Ordering::Relaxed);
            },
            |_| {},
            None,
        )
        .map_err(|e| e.to_string())
}
