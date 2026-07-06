#pragma once
// General MIDI instrument model.
// 128 instruments in 16 families of 8. Program Change N selects instrument N
// on any GM-aware host (portable/plug-and-play). Family f covers programs
// f*8 .. f*8+7.

const char* const GM_FAMILY_NAMES[16] = {
  "Piano", "Chromatic Perc", "Organ", "Guitar",
  "Bass", "Strings", "Ensemble", "Brass",
  "Reed", "Pipe", "Synth Lead", "Synth Pad",
  "Synth FX", "Ethnic", "Percussive", "Sound FX"
};

// Names kept to <=10 chars so they render at the large font on the OLED.
const char* const GM_NAMES[128] = {
  // 0 Piano
  "Grand Pno", "Bright Pno", "ElGrandPno", "Honkytonk",
  "E.Piano 1", "E.Piano 2", "Harpsichd", "Clavi",
  // 8 Chromatic Percussion
  "Celesta", "Glocken", "Music Box", "Vibraphon",
  "Marimba", "Xylophone", "Tub.Bells", "Dulcimer",
  // 16 Organ
  "Drawbar Og", "Perc Organ", "Rock Organ", "Church Og",
  "Reed Organ", "Accordion", "Harmonica", "TangoAccrd",
  // 24 Guitar
  "Nylon Gtr", "Steel Gtr", "Jazz Gtr", "Clean Gtr",
  "Muted Gtr", "OvrdrvGtr", "Dist Gtr", "Gtr Harm",
  // 32 Bass
  "Aco.Bass", "FingerBass", "Pick Bass", "Fretless",
  "Slap Bs 1", "Slap Bs 2", "Syn Bass1", "Syn Bass2",
  // 40 Strings
  "Violin", "Viola", "Cello", "Contrabass",
  "Tremolo St", "Pizz Str", "Harp", "Timpani",
  // 48 Ensemble
  "Str Ens 1", "Str Ens 2", "SynStr 1", "SynStr 2",
  "Choir Aah", "Voice Ooh", "Syn Voice", "Orch Hit",
  // 56 Brass
  "Trumpet", "Trombone", "Tuba", "Mute Tpt",
  "Fr. Horn", "Brass Sec", "SynBrass1", "SynBrass2",
  // 64 Reed
  "Sop Sax", "Alto Sax", "Tenor Sax", "Bari Sax",
  "Oboe", "Eng Horn", "Bassoon", "Clarinet",
  // 72 Pipe
  "Piccolo", "Flute", "Recorder", "Pan Flute",
  "Blown Btl", "Shakuhach", "Whistle", "Ocarina",
  // 80 Synth Lead
  "Squ Lead", "Saw Lead", "Calliope", "Chiff Ld",
  "Charang", "Voice Ld", "5th Lead", "Bass+Lead",
  // 88 Synth Pad
  "NewAge Pad", "Warm Pad", "Poly Pad", "Choir Pad",
  "Bowed Pad", "Metal Pad", "Halo Pad", "Sweep Pad",
  // 96 Synth FX
  "Rain FX", "Soundtrk", "Crystal", "Atmosphere",
  "Brightnes", "Goblins", "Echoes", "Sci-Fi",
  // 104 Ethnic
  "Sitar", "Banjo", "Shamisen", "Koto",
  "Kalimba", "Bagpipe", "Fiddle", "Shanai",
  // 112 Percussive
  "TinkleBell", "Agogo", "SteelDrum", "Woodblock",
  "Taiko Drm", "Mel. Tom", "Synth Drm", "Rev Cymbl",
  // 120 Sound FX
  "Fret Noise", "Breath", "Seashore", "Bird Twt",
  "Telephone", "Helicoptr", "Applause", "Gunshot"
};
