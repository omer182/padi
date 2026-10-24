// Mirrors the firmware's tables (macropad-midi.ino / catalog.h) so the app can
// show what every pad plays. Keep in sync until the app pushes these to the pad.

export type DeviceState = Record<string, number>;

export const NOTE_NAMES = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
export const noteName = (n: number) => `${NOTE_NAMES[n % 12]}${Math.floor(n / 12) - 1}`;

export enum Style { Scaled, Chromatic, Chord, Generative, Omni, Drums }
export const STYLE_NAMES = ["Scale", "Chromatic", "Chords", "Generative", "Strum", "Drums"];

export const SCALE_NAMES = ["Major", "Minor", "Blues", "Harmonic", "Pent Maj", "Pent Min", "Lydian", "Chromatic"];
const SCALES = [
  [0, 2, 4, 5, 7, 9, 11, 12, 14, 16, 17, 19, 21, 23, 24, 26],
  [0, 2, 3, 5, 7, 8, 10, 12, 14, 15, 17, 19, 20, 22, 24, 26],
  [0, 3, 5, 6, 7, 10, 12, 15, 17, 18, 19, 22, 24, 27, 29, 30],
  [0, 2, 3, 5, 7, 8, 11, 12, 14, 15, 17, 19, 20, 23, 24, 26],
  [0, 2, 4, 7, 9, 12, 14, 16, 19, 21, 24, 26, 28, 31, 33, 36],
  [0, 3, 5, 7, 10, 12, 15, 17, 19, 22, 24, 27, 29, 31, 34, 36],
  [0, 2, 4, 6, 7, 9, 11, 12, 14, 16, 18, 19, 21, 23, 24, 26],
  [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
];

export const CHORD_NAMES = ["Major", "Minor", "Blues", "Harmonic", "Open", "Minor", "Jazz 7ths", "Diatonic"];
const CHORD_MODE_SCALES = [0, 1, 2, 3, 0, 1, 0, 0];
const X = 127; // "no note"
const CHORD_VOICINGS = [
  [[-12, 7, 12, 14, 16], [-10, 5, 7, 12, 16], [-8, 7, 11, 12, 16], [-7, 7, 9, 12, 16], [-5, 7, 11, 12, 14], [-3, 7, 11, 12, 16], [-1, 7, 11, 12, 14], [0, 7, 12, 14, 16]],
  [[-12, 3, 7, 12, 15], [-10, 5, 10, 14, 17], [-9, 3, 7, 12, 15], [-7, 5, 10, 12, 17], [-5, 7, 12, 14, 19], [-4, 3, 8, 12, 15], [-2, 5, 10, 14, 17], [0, 7, 12, 15, 19]],
  [[0, 4, 7, 10, X], [2, 5, 9, 12, X], [4, 7, 11, 14, X], [5, 9, 12, 15, X], [7, 11, 14, 17, X], [9, 12, 16, 19, X], [10, 14, 17, 20, X], [0, 4, 7, 10, X]],
  [[0, 3, 7, 12, X], [2, 5, 8, 12, X], [3, 7, 11, 14, X], [5, 8, 12, 15, X], [7, 11, 14, 17, X], [8, 12, 15, 19, X], [11, 14, 17, 20, X], [3, 7, 12, 15, X]],
  [[-12, 0, 7, 16, 19], [-10, 2, 9, 17, 21], [-8, 4, 11, 19, 23], [-7, 5, 12, 21, 24], [-5, 7, 14, 23, 26], [-3, 9, 16, 24, 28], [-1, 12, 19, 26, 31], [0, 12, 19, 28, 31]],
  [[0, 7, 12, 15, X], [2, 10, 14, 17, X], [3, 7, 10, 15, X], [5, 8, 12, 17, X], [7, 10, 14, 19, X], [3, 8, 12, 15, X], [5, 10, 14, 17, X], [7, 12, 15, 19, X]],
  [[4, 11, 14, 19, X], [5, 12, 14, 21, X], [7, 11, 16, 19, X], [9, 12, 16, 21, X], [11, 16, 17, 23, X], [12, 16, 19, 24, X], [14, 17, 21, 26, X], [16, 19, 23, 28, X]],
  [[0, 4, 7, 12, X], [2, 5, 9, 14, X], [4, 7, 11, 16, X], [5, 9, 12, 17, X], [7, 11, 14, 19, X], [9, 12, 16, 21, X], [11, 14, 17, 23, X], [12, 16, 19, 24, X]],
];
const OMNI = [
  [-12, 0, 4, 7, 12, 16, 19, 24], [-10, 2, 5, 9, 14, 17, 21, 26], [-8, 4, 7, 11, 16, 19, 23, 28], [-7, 5, 9, 12, 17, 21, 24, 29],
  [-5, 7, 11, 14, 19, 23, 26, 31], [-3, 9, 12, 16, 21, 24, 28, 33], [-1, 11, 14, 18, 23, 26, 30, 35], [0, 12, 16, 19, 24, 28, 31, 36],
];
const OMNI_CHORDS = ["I", "ii", "iii", "IV", "V", "vi", "vii°", "I'"];

export const DRUM_NOTES = [36, 38, 42, 46, 39, 37, 44, 54, 45, 47, 48, 50, 49, 51, 56, 70];
const DRUM_NAMES = ["Kick", "Snare", "Cl Hat", "Op Hat", "Clap", "Rim", "Pedal", "Tamb", "Lo Tom", "Mid Tom", "Hi-Mid", "Hi Tom", "Crash", "Ride", "Cowbell", "Maracas"];

export const FX_NAMES = ["Filter", "Reso", "Pitch", "Reverb", "Chorus", "Delay", "Phaser", "Drive"];
export const BEAT_NAMES = ["Off", "RnB", "HipHop", "BoomBap", "Trap", "House", "Funk", "Disco"];

export const CAT_NAMES = ["Keys", "Pads", "Leads", "Bass", "Plucks", "Poly", "Arps", "Space", "SynBass", "Signat"];
export const CAT_PATCH_NAMES = [
  ["GrndPno", "Upright", "SoftPno", "EP 1", "DX EP", "Suitcs", "Organ", "Church"],
  ["WarmPad", "Pad 1", "Bright", "Distant", "Ghost", "HrmSwp", "BellPad", "Ooh"],
  ["Lead 1", "Lead 2", "CrspPWM", "Digi It", "Butter", "Fairy", "FM Rock", "Dome"],
  ["Bass 1", "E-Bass", "Rubber", "FMBass1", "DeepEnd", "DstBass", "Digibas", "PnoBass"],
  ["Bell 1", "FMPluck", "E-Guitr", "Clean", "Diamnds", "DlyDncr", "FantBel", "80sGlis"],
  ["Jupitr8", "Juno60", "Triton", "FastPly", "FM Poly", "FatCurt", "AhhPoly", "Gentle"],
  ["Acid 1", "Octave", "BellSeq", "MultiSq", "FM Seq", "GameOn", "GateChd", "FifthSq"],
  ["DeepNot", "SpaceAd", "SpcCadt", "Aliens", "Stardst", "GlasVox", "Prtty5", "Vinyl"],
  ["Chonky", "Slap", "Synthwv", "JunoFir", "HoovrFM", "Electro", "Crystal", "Screamy"],
  ["WarmJup", "BluLine", "Appalld", "VivaTrn", "80sBrss", "TrncLd", "BluePad", "MusicBx"],
];

export const GM_FAMILIES = [
  "Piano", "Chromatic Perc", "Organ", "Guitar", "Bass", "Strings", "Ensemble", "Brass",
  "Reed", "Pipe", "Synth Lead", "Synth Pad", "Synth FX", "Ethnic", "Percussive", "Sound FX",
];

/** Firmware defaults until the pad reports its state. */
export const DEFAULT_STATE: DeviceState = { st: 0, sc: 0, cm: 0, ic: 0, cat: 0, pa: 0, fx: 0, fv: 0, bt: 0, mn: 0, pg: 1, tr: 0, oc: -1, ve: 100, ch: 1, rt: 60 };

const clampNote = (n: number) => Math.max(0, Math.min(127, n));

/** MIDI channel (0-based) a pad plays on. */
export const padChannel = (s: DeviceState) => (s.st === Style.Drums ? 9 : (s.ch ?? 1) - 1);

/** The notes a pad sounds in the current mode (empty = selects, doesn't sound). */
export function padNotes(s: DeviceState, pad: number): number[] {
  const base = s.rt + s.tr + s.oc * 12;
  switch (s.st as Style) {
    case Style.Scaled: return [clampNote(base + SCALES[s.sc][pad])];
    case Style.Chromatic: return [clampNote(base + pad)];
    case Style.Chord:
      if (pad < 8) return CHORD_VOICINGS[s.cm][pad].filter((i) => i !== X).map((i) => clampNote(base + i));
      return [clampNote(base + 12 + SCALES[CHORD_MODE_SCALES[s.cm]][pad - 8])];
    case Style.Omni: return pad < 8 ? [] : [clampNote(base + OMNI[0][pad - 8])];
    case Style.Drums: return [DRUM_NOTES[pad]];
    default: return [];
  }
}

export interface PadLabel { main: string; sub?: string; role?: "chord" | "melody" | "drum" | "nav" | "item" | "selected" }

const ROMAN = ["I", "ii", "iii", "IV", "V", "vi", "vii°", "I'"];

export function padLabel(s: DeviceState, pad: number): PadLabel {
  if (s.mn) return menuLabel(s, pad);
  const notes = padNotes(s, pad);
  switch (s.st as Style) {
    case Style.Drums: return { main: DRUM_NAMES[pad], sub: noteName(DRUM_NOTES[pad]), role: "drum" };
    case Style.Chord:
      if (pad < 8) return { main: ROMAN[pad], sub: notes.map((n) => NOTE_NAMES[n % 12]).join(" "), role: "chord" };
      return { main: noteName(notes[0]), role: "melody" };
    case Style.Omni:
      if (pad < 8) return { main: OMNI_CHORDS[pad], sub: "hold", role: "chord" };
      return { main: `str ${pad - 7}`, role: "melody" };
    case Style.Generative: return { main: "·" };
    default: return { main: noteName(notes[0]) };
  }
}

function menuLabel(s: DeviceState, pad: number): PadLabel {
  if (pad === 12) return { main: "◀", sub: "page", role: "nav" };
  if (pad === 13) return { main: "▶", sub: "page", role: "nav" };
  if (pad === 14 || pad === 15) return { main: "Exit", role: "nav" };
  const page = s.pg;
  const item = (name: string, sel: boolean): PadLabel => ({ main: name, role: sel ? "selected" : "item" });
  if (page === 1) {
    if (pad < 8) return item(CAT_PATCH_NAMES[s.ic][pad], s.ic === s.cat && pad === s.pa);
    if (pad === 8) return { main: "◀ cat", role: "nav" };
    if (pad === 9) return { main: "cat ▶", role: "nav" };
  } else if (page === 2) {
    if (pad < BEAT_NAMES.length) return item(BEAT_NAMES[pad], pad === s.bt);
  } else if (page === 3) {
    if (pad < 8) return item(SCALE_NAMES[pad], s.st !== Style.Drums && pad === s.sc);
    if (pad === 8) return item("Drums", s.st === Style.Drums);
  } else if (page === 4) {
    if (pad < 8) return item(CHORD_NAMES[pad], pad === s.cm);
  } else if (pad < 8) {
    return item(FX_NAMES[pad], pad === s.fx);
  }
  return { main: "" };
}

export const MENU_PAGES = ["", "Instruments", "Beats", "Scales", "Chords", "Knob FX"];

export function modeName(s: DeviceState): string {
  if (s.st === Style.Drums) return "Drums";
  if (s.st === Style.Chord || s.st === Style.Omni) return `${STYLE_NAMES[s.st]} · ${CHORD_NAMES[s.cm]}`;
  if (s.st === Style.Chromatic) return "Chromatic";
  return `${STYLE_NAMES[s.st]} · ${SCALE_NAMES[s.sc]}`;
}
