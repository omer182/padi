// The README demo video's script: one timeline that drives both the picture
// (capture.mjs, via the UI demo mode) and the sound (cargo run --example
// demo_audio). Writes timeline.json next to this file.
//
// 140 BPM, A minor, 12 bars (~20.6 s):
//   bars 0-1   Saw Lead riff on the pads, knob opens the filter, looper records L1
//   bars 2-5   808 kit + gliding 808 sub enter, looper overdubs L2, echo goes up
//   bars 6-9   sampler: Robot / Chipmunk / Hey! / Pop pads over the beat, L3
//   bars 10-11 lead returns, filter closes, final 808 hit
import fs from "node:fs";

const BPM = 140;
const S16 = 60 / BPM / 4;          // one 16th note
const BAR = S16 * 16;
const BARS = 12;
const SECONDS = +(BAR * BARS).toFixed(3);
const t16 = (bar, step) => +((bar * 16 + step) * S16).toFixed(4);

// Minor scale from A3 (the pad's Scale/Minor with root A): pad -> MIDI note.
const MINOR = [0, 2, 3, 5, 7, 8, 10, 12, 14, 15, 17, 19, 20, 22, 24, 26];
const padNote = (p) => 57 + MINOR[p];

const tracks = { lead: [], drums: [], bass: [], robot: [], chip: [], hit: [], pop: [], riser: [] };
const pads = []; // visual presses: { t, d, pad }
const note = (track, t, d, k, v = 100) => tracks[track].push({ t, d: +d.toFixed(4), k, v });

// ---- lead: 2-bar riff in 8ths (pads), bars 0-5 and 10-11 ----
const RIFF = [7, 9, 11, 7, 6, 11, 10, 9, 5, 7, 9, 5, 4, 6, 8, 11];
for (const bar of [0, 2, 4, 10]) {
  RIFF.forEach((p, i) => {
    const b = bar + Math.floor(i / 8), step = (i % 8) * 2;
    const last = bar === 10 && i === RIFF.length - 1;
    const t = t16(b, step), d = last ? BAR * 0.9 : S16 * 1.6;
    note("lead", t, d, padNote(p), 96 + (i % 2) * 12);
    if (bar < 6 || bar >= 10) pads.push({ t, d: Math.min(d, S16 * 1.6), pad: p });
  });
}

// ---- drums: 808/909 kit, trap pattern from bar 2 ----
const KICK = 36, CLAP = 39, SNARE = 38, HAT = 42, OPEN = 46;
for (let bar = 2; bar < BARS; bar++) {
  const lastBar = bar === BARS - 1;
  const kicks = lastBar ? [0] : bar % 2 ? [0, 7, 10] : [0, 10, 13];
  for (const s of kicks) note("drums", t16(bar, s), 0.2, KICK, 120);
  if (!lastBar) {
    note("drums", t16(bar, 8), 0.2, CLAP, 112);
    note("drums", t16(bar, 8), 0.2, SNARE, 70);
    for (let s = 0; s < 16; s += 2) note("drums", t16(bar, s), 0.08, s === 14 && bar % 4 === 3 ? OPEN : HAT, s % 4 ? 62 : 88);
    if (bar % 2) for (let k = 0; k < 6; k++) note("drums", t16(bar, 12) + k * S16 * 2 / 3, 0.05, HAT, 50 + k * 8); // triplet roll
  }
  // 808 sub on every kick; root follows the riff's harmony (Am, F, C, G).
  const roots = [33, 29, 36, 31];
  kicks.forEach((s, j) => {
    const root = roots[(bar * 2 + (s >= 8 ? 1 : 0)) % 4];
    const glide = j === kicks.length - 1 && !lastBar ? 12 : 0; // last hit of the bar slides up
    note("bass", t16(bar, s), lastBar ? BAR : S16 * (j === kicks.length - 1 ? 5 : 3), root, glide);
  });
}

// ---- sampler section: bars 6-9 ----
const SAMPLER = [ // [step, track, pad, notes]
  [[0, "robot", 5, [57, 52]], [6, "chip", 7, [93, 96, 100]], [8, "hit", 2, [69]], [12, "robot", 5, [60, 55]]],
  [[0, "pop", 11, [76]], [3, "pop", 11, [79]], [4, "robot", 5, [64, 57]], [8, "hit", 2, [72]], [10, "chip", 7, [96, 100, 103]], [14, "robot", 5, [57]]],
];
for (let bar = 6; bar < 10; bar++) {
  for (const [step, tr, pad, keys] of SAMPLER[bar % 2]) {
    const t = t16(bar, step);
    keys.forEach((k, i) => note(tr, t + i * S16 * (tr === "chip" ? 0.5 : 1.5), tr === "robot" ? S16 * 1.4 : S16, k, 110));
    pads.push({ t, d: S16 * 1.6, pad });
  }
  for (const s of bar % 2 ? [0, 7, 10] : [0, 10, 13]) pads.push({ t: t16(bar, s), d: S16, pad: 0 }); // "Beatbox kick"
}

// ---- risers into the drop and into the sampler section ----
for (const bar of [1, 5, 9]) note("riser", t16(bar, 0), BAR, 60, 110);

// ---- automation: [time, value] ----
const auto = {
  cutoff: [[0, 0.22], [BAR * 1.75, 1], [t16(11, 8), 1], [SECONDS, 0.18]],   // the pad knob (Filter)
  echo: [[0, 0.12], [t16(4, 0), 0.12], [t16(4, 6), 0.38], [t16(10, 0), 0.38], [t16(11, 0), 0.55]],
};

// ---- looper + UI keyframes (applied in order, each merges into the state) ----
const LOOP = BAR * 2;
const loop = {
  len: LOOP,
  rec: [[0, LOOP], [t16(2, 0), t16(4, 0)], [t16(6, 0), t16(8, 0)]],
  layers: [[0, 0], [t16(2, 0), 1], [t16(4, 0), 2], [t16(8, 0), 3]],
};
const lead = { font: 0, bank: 0, program: 81, name: "Saw Lead", mySound: null };
const ui = [
  { t: 0, tab: "Sounds", device: { st: 0, sc: 1, rt: 57, oc: 0, tr: 0, fx: 0, bt: 0 }, sound: lead,
    drumKit: { font: 0, bank: 128, program: 25, name: "808/909", mySound: null }, studio: { active: false } },
  { t: t16(6, 0), tab: "My kits", device: { st: 6 }, studio: { active: true } },
  { t: t16(10, 0), tab: "Sounds", device: { st: 0 }, studio: { active: false } },
];

const out = { bpm: BPM, seconds: SECONDS, s16: S16, tracks, pads, auto, loop, ui };
fs.writeFileSync(new URL("./timeline.json", import.meta.url), JSON.stringify(out, null, 1));
console.log(`timeline: ${SECONDS}s, ${Object.values(tracks).reduce((n, t) => n + t.length, 0)} notes, ${pads.length} pad presses`);
