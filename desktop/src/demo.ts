// Browser demo mode: when the UI is opened in a plain browser (no Tauri
// backend), answer the app's backend calls with sample data so the real UI can
// be explored and captured for the README. Never loaded inside the app.
//
//   npm run dev  ->  http://localhost:1420/?scene=main | sampler | armed
//   window.padiDemo.frame(i)   step a scripted performance (pads, notes, loop)
import { mockIPC } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";

const scene = new URLSearchParams(location.search).get("scene") ?? "main";

const GM = [
  "Grand Piano", "Bright Grand Piano", "Electric Grand Piano", "Honky-Tonk Piano", "Tine Electric Piano", "FM Electric Piano", "Harpsichord", "Clavinet",
  "Celeste", "Glockenspiel", "Music Box", "Vibraphone", "Marimba", "Xylophone", "Tubular Bells", "Dulcimer",
  "Tonewheel Organ", "Percussive Organ", "Rock Organ", "Pipe Organ", "Reed Organ", "Accordion", "Harmonica", "Bandoneon",
  "Nylon Guitar", "Steel Guitar", "Jazz Guitar", "Clean Guitar", "Muted Guitar", "Overdrive Guitar", "Distortion Guitar", "Guitar Harmonics",
  "Acoustic Bass", "Finger Bass", "Pick Bass", "Fretless Bass", "Slap Bass 1", "Slap Bass 2", "Synth Bass 1", "Synth Bass 2",
  "Violin", "Viola", "Cello", "Contrabass", "Tremolo Strings", "Pizzicato Strings", "Orchestral Harp", "Timpani",
  "String Ensemble", "Slow Strings", "Synth Strings 1", "Synth Strings 2", "Choir Aahs", "Voice Oohs", "Synth Voice", "Orchestra Hit",
  "Trumpet", "Trombone", "Tuba", "Muted Trumpet", "French Horns", "Brass Section", "Synth Brass 1", "Synth Brass 2",
  "Soprano Sax", "Alto Sax", "Tenor Sax", "Baritone Sax", "Oboe", "English Horn", "Bassoon", "Clarinet",
  "Piccolo", "Flute", "Recorder", "Pan Flute", "Bottle Chiff", "Shakuhachi", "Whistle", "Ocarina",
  "Square Lead", "Saw Lead", "Calliope Lead", "Chiffer Lead", "Charang", "Solo Vox", "5th Saw Lead", "Bass & Lead",
  "Fantasia", "Warm Pad", "Polysynth", "Space Voice", "Bowed Glass", "Metal Pad", "Halo Pad", "Sweep Pad",
  "Ice Rain", "Soundtrack", "Crystal", "Atmosphere", "Brightness", "Goblin", "Echo Drops", "Star Theme",
  "Sitar", "Banjo", "Shamisen", "Koto", "Kalimba", "Bagpipe", "Fiddle", "Shanai",
  "Tinkle Bell", "Agogo", "Steel Drums", "Woodblock", "Taiko Drum", "Melodic Tom", "Synth Drum", "Reverse Cymbal",
  "Fret Noise", "Breath Noise", "Seashore", "Bird Tweet", "Telephone", "Helicopter", "Applause", "Gunshot",
];
const FONTS = ["GeneralUser GS", "Spanish Guitar", "Steel String Guitar", "Electric Guitar Clean", "Grand Piano YDP",
  "FM Electric Piano", "SciFi FX", "Goblins FX", "Soundtrack FX", "Crystal FX"];
const PACK_NAMES = ["", "Classical guitar", "Steel String Guitar", "EGuitar Clean", "Grand Piano", "FM Piano 1", "Synth Sci-Fi",
  "Synth Goblins", "Synth Soundtrack", "Synth Crystal"];
const KITS = ["Standard 1", "Standard 2", "Room", "Power", "Electronic", "808/909", "Dance", "Jazz", "Brush", "Orchestral"];
const presets = [
  ...GM.map((name, program) => ({ font: 0, bank: 0, program, name })),
  ...KITS.map((name, i) => ({ font: 0, bank: 128, program: [0, 1, 8, 16, 24, 25, 26, 32, 40, 48][i], name })),
  ...PACK_NAMES.slice(1).map((name, i) => ({ font: i + 1, bank: 0, program: 0, name })),
];

const params = { reverb: 58, chorus: 18, brightness: 112, volume: 100, pan: 64, octave: 0, echo: 0.2 };
const sampler = scene !== "main";
const device: Record<string, number> = {
  st: sampler ? 6 : 0, sc: 0, cm: 0, ic: 0, cat: 0, pa: 0, fx: 3, fv: 74, bt: 1, mn: 0, pg: 1, tr: 0, oc: -1, ve: 100, ch: 1, rt: 60,
};
const status = {
  engine: { ok: true, output: "Speakers", sampleRate: 48000, error: null },
  picoMidi: true, picoSerial: true, serialPaused: false,
  sound: { font: 1, bank: 0, program: 0, name: "Warm Nylon", params, mySound: "s1" },
  drumKit: { font: 0, bank: 128, program: 0, name: "Standard 1", params: { ...params, echo: 0 }, mySound: null },
  volume: 0.8,
  device,
};

// A recorded "funny noises" kit: envelope shapes stand in for real takes.
function peaks(seed: number, shape: "hit" | "swell" | "wobble" | "double") {
  return Array.from({ length: 40 }, (_, i) => {
    const t = i / 39, n = 0.75 + 0.25 * Math.sin(seed * 9.1 + i * 2.3);
    const env = shape === "hit" ? Math.exp(-t * 5)
      : shape === "swell" ? Math.sin(t * Math.PI)
      : shape === "wobble" ? (0.55 + 0.45 * Math.sin(t * 18)) * (1 - t * 0.7)
      : Math.max(Math.exp(-t * 9), Math.exp(-Math.abs(t - 0.45) * 12));
    return Math.max(0.05, env * n);
  });
}
const takes: [string, "oneshot" | "hold" | "loop", "none" | "robot" | "crush", number, number, "hit" | "swell" | "wobble" | "double"][] = [
  ["Beatbox kick", "oneshot", "none", 0, 0, "hit"], ["Snap", "oneshot", "none", 0, 0, "hit"], ["Hey!", "oneshot", "none", 0.3, 0, "double"], ["Clap clap", "oneshot", "none", 0, 0, "double"],
  ["Boing", "oneshot", "none", 0.2, 5, "wobble"], ["Robot hello", "hold", "robot", 0.25, 0, "swell"], ["Ooh", "hold", "none", 0.4, 0, "swell"], ["Chipmunk laugh", "oneshot", "none", 0, 12, "wobble"],
  ["Meow", "oneshot", "crush", 0, 0, "swell"], ["Monster", "oneshot", "none", 0.15, -12, "wobble"], ["Whistle", "loop", "none", 0.3, 0, "swell"], ["Pop", "oneshot", "none", 0, 0, "hit"],
];
const studio = {
  kitId: "funny-noises", kitName: "Funny Noises",
  kits: [{ id: "funny-noises", name: "Funny Noises" }, { id: "kitchen-drums", name: "Kitchen Drums" }, { id: "my-voice", name: "My Voice" }],
  sounds: [
    { id: "s1", name: "Warm Nylon", fontName: "Spanish Guitar", bank: 0, program: 0, params },
    { id: "s2", name: "Space Choir", fontName: "GeneralUser GS", bank: 0, program: 91, params },
    { id: "s3", name: "Lo-fi Keys", fontName: "FM Electric Piano", bank: 0, program: 0, params },
  ],
  pads: Array.from({ length: 16 }, (_, i) => {
    const t = takes[i];
    if (!t) return null;
    return { name: t[0], file: `pad${i + 1}.wav`, mode: t[1], fx: t[2], echo: t[3], pitch: t[4], gain: 1, reverse: false,
      seconds: 0.4 + (i % 5) * 0.3, peaks: peaks(i, t[5]) };
  }),
  active: sampler, armed: scene === "armed", recordingPad: scene === "armed" ? 13 : null,
  canUndo: true, canRedo: false, bpm: 96, bars: 2, metronome: true,
  layerMutes: [false, false, true], loopRedo: 0, sessionFile: null, exportsDir: "C:\\Users\\you\\Music\\PADI",
};

mockIPC((cmd, args) => {
  switch (cmd) {
    case "get_status": return status;
    case "get_log": return ["Sounds loaded: GeneralUser GS (287 presets)", "Pad serial connected", "Pad MIDI connected", "Recorded 0.6s onto pad 6"];
    case "get_presets": return { fonts: FONTS, presets };
    case "get_studio": return studio;
    case "plugin:autostart|is_enabled": return false;
    default:
      if (cmd === "studio_pad" || cmd === "play_note") return null;
      console.debug("[demo] ignored", cmd, args);
      return null;
  }
}, { shouldMockEvents: true });

// ---- scripted performance for captures -------------------------------------
// Main scene: a little riff in C major (pads = notes). Sampler: a beat.
const MAJOR = [0, 2, 4, 5, 7, 9, 11, 12, 14, 16, 17, 19, 21, 23, 24, 26];
const RIFF = [[0, 4], [2], [4, 7], [2], [5, 9], [4], [7, 11], [9, 12], [7], [4, 8], [2], [0, 4, 7]];
const BEAT = [[0, 6], [3], [1, 10], [3], [0, 4], [3, 7], [1, 5], [3]];
let held: number[] = [];
const FRAMES_PER_STEP = 3;

async function setPads(next: number[]) {
  const base = 60 - 12; // root C4, octave -1
  for (const p of held.filter((p) => !next.includes(p))) {
    await emit("pad", { pad: p, down: false });
    if (!sampler) await emit("midi", { kind: "off", ch: 0, a: base + MAJOR[p], b: 0 });
  }
  for (const p of next.filter((p) => !held.includes(p))) {
    await emit("pad", { pad: p, down: true });
    if (!sampler) await emit("midi", { kind: "on", ch: 0, a: base + MAJOR[p], b: 100 });
  }
  held = next;
}

async function frame(i: number, total = 48) {
  const seq = sampler ? BEAT : RIFF;
  const step = Math.floor(i / FRAMES_PER_STEP) % seq.length;
  const gap = i % FRAMES_PER_STEP === FRAMES_PER_STEP - 1; // short release between hits
  await setPads(gap ? [] : seq[step]);
  await emit("transport", {
    pos: ((i / total) * 0.5 + 0.18) % 1, seconds: 5, playing: true, recording: false, countinBeats: 0,
    mic: scene === "armed" ? 0.35 + 0.3 * Math.abs(Math.sin(i * 0.9)) : 0,
  });
}

(window as unknown as { padiDemo: unknown }).padiDemo = { frame, setPads, scene };
// A still frame for screenshots: something held, loop mid-way.
setTimeout(() => frame(sampler ? 12 : 6), 300);
