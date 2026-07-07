// ---------------------------------------------------------------------------
//  Surge instrument catalog (device side = display names only).
//  6 categories x 8 patches. The device sends Bank Select (CC0 = category)
//  then Program Change (= patch index); the PC bridge maps (cat,patch) to the
//  matching Surge .fxp and loads it over OSC.  Keep names <= 7 chars so they
//  fit the OLED menu columns.  Order MUST match CATALOG[][] in the bridge.
// ---------------------------------------------------------------------------
#pragma once

#define CAT_COUNT 10
#define CAT_SIZE  8

const char* CAT_NAMES[CAT_COUNT] = {
  "Keys", "Pads", "Leads", "Bass", "Plucks", "Poly",
  "Arps", "Space", "SynBass", "Signat"
};

const char* CAT_PATCH_NAMES[CAT_COUNT][CAT_SIZE] = {
  // Keys  (slot 0 = Grand Piano = boot default)
  { "GrndPno", "Upright", "SoftPno", "EP 1", "DX EP", "Suitcs", "Organ", "Church" },
  // Pads
  { "WarmPad", "Pad 1", "Bright", "Distant", "Ghost", "HrmSwp", "BellPad", "Ooh" },
  // Leads
  { "Lead 1", "Lead 2", "CrspPWM", "Digi It", "Butter", "Fairy", "FM Rock", "Dome" },
  // Bass
  { "Bass 1", "E-Bass", "Rubber", "FMBass1", "DeepEnd", "DstBass", "Digibas", "PnoBass" },
  // Plucks
  { "Bell 1", "FMPluck", "E-Guitr", "Clean", "Diamnds", "DlyDncr", "FantBel", "80sGlis" },
  // Poly
  { "Jupitr8", "Juno60", "Triton", "FastPly", "FM Poly", "FatCurt", "AhhPoly", "Gentle" },
  // Arps  (self-playing sequences - hold a note and it runs)
  { "Acid 1", "Octave", "BellSeq", "MultiSq", "FM Seq", "GameOn", "GateChd", "FifthSq" },
  // Space  (atmospheres / cinematic textures)
  { "DeepNot", "SpaceAd", "SpcCadt", "Aliens", "Stardst", "GlasVox", "Prtty5", "Vinyl" },
  // SynBass  (boutique 3rd-party basses)
  { "Chonky", "Slap", "Synthwv", "JunoFir", "HoovrFM", "Electro", "Crystal", "Screamy" },
  // Signat  (showpiece keys/pads/leads)
  { "WarmJup", "BluLine", "Appalld", "VivaTrn", "80sBrss", "TrncLd", "BluePad", "MusicBx" },
};
