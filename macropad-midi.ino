/*
 * Macropad MIDI Controller - v1.0
 */

#include <Adafruit_TinyUSB.h>
#include <MIDI.h>
#include <hardware/flash.h>
#include <hardware/sync.h>
#include <Wire.h>
#include <Adafruit_GFX.h>
#include <Adafruit_SSD1306.h>
#include "gm_instruments.h"   // GM_NAMES[128], GM_FAMILY_NAMES[16]
#include "catalog.h"          // Surge catalog: CAT_NAMES[], CAT_PATCH_NAMES[][]

Adafruit_USBD_MIDI usb_midi;
MIDI_CREATE_INSTANCE(Adafruit_USBD_MIDI, usb_midi, MIDI);

const byte PAD_PINS[16] = {0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15};
const byte LED_PIN = 25;

// ---- OLED (SSD1306 128x64 I2C, SDA=GP16 SCL=GP17) ----
#define OLED_W 128
#define OLED_H 64
#define OLED_ADDR 0x3C
Adafruit_SSD1306 display(OLED_W, OLED_H, &Wire, -1);
bool displayReady = false;
bool displayDirty = true;
unsigned long lastDisplayDraw = 0;

// ---- Knob FX (pot on GP26): the knob controls a selectable MIDI effect, NOT
//   volume (volume is set on the host now). It always sends standard MIDI
//   (CC or pitch bend) so it stays plug-and-play on any Mac/PC/synth. ----
#define POT_PIN 26
const unsigned long POT_INTERVAL = 15;
const int POT_RAW_LO = 470;        // raw at/below here = 127 (this pot: ~433 max end)
const int POT_RAW_HI = 1010;       // raw at/above here = 0   (this pot: ~1022 min end)

const byte FX_PITCH = 255;          // sentinel: use pitch bend instead of a CC
struct KnobFx { const char* name; byte cc; byte neutral; };
const KnobFx FX[8] = {
  //  name       CC        neutral (value that = "effect off")
  { "Filter",   74,       127 },    // CC74 cutoff (open)   [GM/MPE standard]
  { "Reso",     71,       0   },    // CC71 resonance       [standard]
  { "Pitch",    FX_PITCH, 64  },    // pitch bend (64=center)[universal]
  { "Reverb",   91,       0   },    // CC91 reverb depth    [GM standard]
  { "Chorus",   93,       0   },    // CC93 chorus depth    [GM standard]
  { "Delay",    85,       0   },    // CC85 (mappable)
  { "Phaser",   86,       0   },    // CC86 (mappable)
  { "Drive",    87,       0   },    // CC87 (mappable)
};
byte knobFx = 0;                    // selected FX index (0..7)
byte fxValue = 0;                   // current knob position for the FX (0..127)

// ---- Note tracking (so we release exactly what's sounding; no 128-msg blast) ----
bool noteActive[128] = {false};

const char* PRESET_NAMES[4] = {"Major", "Chromatic", "Chords", "Generat"};

// ---- Current instrument (Surge catalog: category + patch) ----
byte curCat = 0;           // selected category (0 = Keys)
byte curPatch = 0;         // selected patch in that category (0 = Grand Piano)
byte instCat = 0;          // which category the menu is browsing
unsigned long programAnnounceAt = 0;  // resend selection once host is up

const char* NOTE_NAMES[12] = {"C","C#","D","D#","E","F","F#","G","G#","A","A#","B"};

// ---- Drum-beat presets: looping grooves you play instruments over ----
// 16-step patterns (bit0 = step 0). GM drums on channel 10.
struct Beat { const char* name; byte bpm; uint16_t kick, snare, chat, ohat; };
const Beat BEATS[] = {
  //  name       bpm  kick    snare   closedHat openHat
  {"Off",     0,  0x0000, 0x0000, 0x0000, 0x0000},
  {"RnB",     86, 0x0441, 0x1010, 0x5555, 0x0000},  // laid-back, 8th hats
  {"HipHop",  88, 0x0481, 0x1010, 0x5555, 0x0000},  // kick pickup before snare
  {"BoomBap", 92, 0x0409, 0x1010, 0x5555, 0x0000},  // 90s East-coast
  {"Trap",   140, 0x0041, 0x0100, 0xFFFF, 0x0000},  // half-time, 16th hats
  {"House",  124, 0x1111, 0x1010, 0x0000, 0x4444},  // 4-on-floor + offbeat open hats
  {"Funk",   104, 0x0449, 0x1010, 0xFFFF, 0x0000},  // syncopated kick, 16th hats
  {"Disco",  120, 0x1111, 0x1010, 0x5555, 0x4444},  // 4-floor + hats everywhere
};
const byte BEAT_COUNT = sizeof(BEATS) / sizeof(BEATS[0]);
byte currentBeat = 0;              // 0 = off
byte beatStep = 0;
unsigned long lastStepTime = 0;
const byte GM_DRUM_CH = 10;
const byte KICK = 36, SNARE = 38, CHAT = 42, OHAT = 46;

enum PlayStyle {STYLE_SCALED, STYLE_CHROMATIC, STYLE_CHORD, STYLE_GENERATIVE, STYLE_OMNI_CHORD, STYLE_DRUMS};

// STYLE_DRUMS ("GarageBand/GM drum" mode): the 16 pads play 16 GM drum sounds
// on the drum channel, so every pad hits a real drum in GarageBand / any GM host.
const byte DRUM_NOTES[16] = {
  36, 38, 42, 46,   // Kick, Snare, ClosedHat, OpenHat
  39, 37, 44, 54,   // Clap, Rimshot, PedalHat, Tambourine
  45, 47, 48, 50,   // LowTom, MidTom, HiMidTom, HighTom
  49, 51, 56, 70    // Crash, Ride, Cowbell, Maracas
};

// 8 SCALES - Linear across 16 pads, no repeating notes
const int8_t SCALES[][16] = {
  {0,2,4,5,7,9,11,12,14,16,17,19,21,23,24,26},     // 0: Major (happy, classic)
  {0,2,3,5,7,8,10,12,14,15,17,19,20,22,24,26},     // 1: Minor (sad, emotional)
  {0,3,5,6,7,10,12,15,17,18,19,22,24,27,29,30},    // 2: Blues (soulful)
  {0,2,3,5,7,8,11,12,14,15,17,19,20,23,24,26},     // 3: Harmonic Minor (dramatic, exotic)
  {0,2,4,7,9,12,14,16,19,21,24,26,28,31,33,36},    // 4: Pentatonic Major (can't play wrong)
  {0,3,5,7,10,12,15,17,19,22,24,27,29,31,34,36},   // 5: Pentatonic Minor (easy improv)
  {0,2,4,6,7,9,11,12,14,16,18,19,21,23,24,26},     // 6: Lydian (bright, dreamy)
  {0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15}          // 7: Chromatic (all notes)
};

const char* SCALE_NAMES[] = {"Major","Minor","Blues","Harmonic","Pent Maj","Pent Min","Lydian","Chromatic"};

// Map each chord mode to its ideal scale for the upper 8 pads (organized system)
const byte CHORD_MODE_SCALES[8] = {
  0,  // 0: Major -> Major scale
  1,  // 1: Minor -> Minor scale
  2,  // 2: Blues -> Blues scale
  3,  // 3: Harmonic -> Harmonic Minor scale
  0,  // 4: Open -> Major scale
  1,  // 5: Minor (diatonic) -> Minor scale
  0,  // 6: Jazz 7ths -> Major scale
  0   // 7: Diatonic -> Major scale
};

// 8 CHORD MODES - Reordered as requested
const int8_t CHORD_VOICINGS[][8][5] = {
  // 0. Major - Diatonic with bass roots (keep original - it's perfect!)
  {{-12,7,12,14,16},{-10,5,7,12,16},{-8,7,11,12,16},{-7,7,9,12,16},{-5,7,11,12,14},{-3,7,11,12,16},{-1,7,11,12,14},{0,7,12,14,16}},
  
  // 1. Minor - Natural minor with bass roots
  {{-12,3,7,12,15},{-10,5,10,14,17},{-9,3,7,12,15},{-7,5,10,12,17},{-5,7,12,14,19},{-4,3,8,12,15},{-2,5,10,14,17},{0,7,12,15,19}},
  
  // 2. Blues Chords (dominant 7ths with blue notes)
  {{0,4,7,10,127},{2,5,9,12,127},{4,7,11,14,127},{5,9,12,15,127},{7,11,14,17,127},{9,12,16,19,127},{10,14,17,20,127},{0,4,7,10,127}},
  
  // 3. Harmonic Minor (dramatic, exotic)
  {{0,3,7,12,127},{2,5,8,12,127},{3,7,11,14,127},{5,8,12,15,127},{7,11,14,17,127},{8,12,15,19,127},{11,14,17,20,127},{3,7,12,15,127}},
  
  // 4. Open Voicings (cinematic, wide)
  {{-12,0,7,16,19},{-10,2,9,17,21},{-8,4,11,19,23},{-7,5,12,21,24},{-5,7,14,23,26},{-3,9,16,24,28},{-1,12,19,26,31},{0,12,19,28,31}},
  
  // 5. Minor (diatonic) - Simple minor triads with close inversions
  {{0,7,12,15,127},{2,10,14,17,127},{3,7,10,15,127},{5,8,12,17,127},{7,10,14,19,127},{3,8,12,15,127},{5,10,14,17,127},{7,12,15,19,127}},
  
  // 6. Jazz 7ths - Rootless voicings (3rd, 7th, 9th, 5th)
  {{4,11,14,19,127},{5,12,14,21,127},{7,11,16,19,127},{9,12,16,21,127},{11,16,17,23,127},{12,16,19,24,127},{14,17,21,26,127},{16,19,23,28,127}},
  
  // 7. Diatonic - Standard close voicings, tight groupings
  {{0,4,7,12,127},{2,5,9,14,127},{4,7,11,16,127},{5,9,12,17,127},{7,11,14,19,127},{9,12,16,21,127},{11,14,17,23,127},{12,16,19,24,127}}
};

// 8-note voicings specifically for omni chord mode
const int8_t OMNI_CHORD_VOICINGS[][8] = {
  {-12,0,4,7,12,16,19,24},   // C Major expanded
  {-10,2,5,9,14,17,21,26},   // D minor expanded  
  {-8,4,7,11,16,19,23,28},   // E minor expanded
  {-7,5,9,12,17,21,24,29},   // F Major expanded
  {-5,7,11,14,19,23,26,31},  // G Major expanded
  {-3,9,12,16,21,24,28,33},  // A minor expanded
  {-1,11,14,18,23,26,30,35}, // B diminished expanded
  {0,12,16,19,24,28,31,36}   // C Major octave expanded
};

const char* CHORD_NAMES[] = {"Major","Minor","Blues","Harmonic","Open","Minor","Jazz 7ths","Diatonic"};

// Settings
PlayStyle playStyle = STYLE_SCALED;
byte currentScale = 0;
byte currentChordMode = 0;
int8_t transpose = 0;
int8_t octave = -1;
byte velocity = 100;
byte midiChannel = 1;
byte rootNote = 60;

// Tell the bridge which patch to load: Bank Select (CC0 = category) + Program Change.
void sendInstrument(byte cat, byte patch) {
  MIDI.sendControlChange(0, cat, midiChannel);   // CC0 = Bank Select MSB = category
  MIDI.sendProgramChange(patch, midiChannel);
}

// Use 127 as a marker for "no note" in chord voicings
const int8_t NO_NOTE = 127;

struct Preset {
  PlayStyle style;
  byte scale;
  byte chordMode;
  int8_t transpose;
  int8_t octave;
  byte velocity;
};

Preset presets[4];

// Flash storage (last 4KB of flash)
#define FLASH_TARGET_OFFSET (PICO_FLASH_SIZE_BYTES - FLASH_SECTOR_SIZE)
#define FLASH_MAGIC 0x4D494451  // bump when FlashData layout changes
struct FlashData {
  uint32_t magic;
  Preset presets[4];
  uint8_t lastCat;    // remember the last-selected instrument across reboots
  uint8_t lastPatch;
  uint8_t lastFx;     // remember the knob-FX choice
};

// Generative
unsigned long lastGenNoteTime = 0;
unsigned long nextGenNoteDelay = 0;
byte currentGenNote = 255;
const unsigned long MIN_GEN_DELAY = 800;
const unsigned long MAX_GEN_DELAY = 3000;
const byte GEN_TRIGGER_CHANCE = 70;

// Omni Chord Mode
byte omniChordIndex = 0;  // Currently selected chord (bottom 8 pads)
byte omniChordNotes[8] = {255, 255, 255, 255, 255, 255, 255, 255};  // Track actual MIDI notes for each top pad


// Menu
bool inMenu = false;
byte currentPage = 1;
const byte TOP_ROW[4] = {12,13,14,15};
unsigned long topRowPressTime[4] = {0,0,0,0};
bool topRowPressed = false;
unsigned long padHoldStartTime[16] = {0};
const unsigned long PRESET_SAVE_HOLD_TIME = 2000;

bool padStates[16] = {false};
bool lastPadStates[16] = {false};
unsigned long lastDebounceTime[16] = {0};
const unsigned long DEBOUNCE_DELAY = 20;

void loadPresetsFromFlash() {
  const FlashData* flash_data = (const FlashData*)(XIP_BASE + FLASH_TARGET_OFFSET);
  
  if (flash_data->magic == FLASH_MAGIC) {
    // Valid data found, load it
    for (int i = 0; i < 4; i++) {
      presets[i] = flash_data->presets[i];
    }
    // Restore the last-selected instrument (clamp in case the catalog shrank).
    if (flash_data->lastCat < CAT_COUNT && flash_data->lastPatch < CAT_SIZE) {
      curCat = flash_data->lastCat;
      curPatch = flash_data->lastPatch;
    }
    if (flash_data->lastFx < 8) knobFx = flash_data->lastFx;   // restore knob-FX choice
  } else {
    // No valid data, use defaults
    presets[0] = {STYLE_SCALED, 0, 0, 0, -1, 100};
    presets[1] = {STYLE_CHROMATIC, 7, 0, 0, -1, 100};  // Chromatic scale
    presets[2] = {STYLE_CHORD, 0, 0, 0, -1, 100};
    presets[3] = {STYLE_GENERATIVE, 0, 0, 0, -1, 80};
  }
}

// Write presets + last instrument to flash (quiet, no LED).
void writeFlash() {
  FlashData data;
  data.magic = FLASH_MAGIC;
  for (int i = 0; i < 4; i++) {
    data.presets[i] = presets[i];
  }
  data.lastCat = curCat;
  data.lastPatch = curPatch;
  data.lastFx = knobFx;

  uint8_t buffer[FLASH_PAGE_SIZE];
  memcpy(buffer, &data, sizeof(FlashData));

  uint32_t ints = save_and_disable_interrupts();
  flash_range_erase(FLASH_TARGET_OFFSET, FLASH_SECTOR_SIZE);
  flash_range_program(FLASH_TARGET_OFFSET, buffer, FLASH_PAGE_SIZE);
  restore_interrupts(ints);
}

void savePresetsToFlash() {
  writeFlash();
  // Flash LED to confirm (explicit preset save only)
  for (int i = 0; i < 10; i++) {
    digitalWrite(LED_PIN, i % 2);
    delay(50);
  }
}

// ---------------------------------------------------------------------------
//  OLED
// ---------------------------------------------------------------------------
void initDisplay() {
  Wire.setSDA(16);
  Wire.setSCL(17);
  Wire.begin();
  if (display.begin(SSD1306_SWITCHCAPVCC, OLED_ADDR)) {
    displayReady = true;
    display.setTextColor(SSD1306_WHITE);
    display.setTextWrap(false);
    display.clearDisplay();
    display.display();
  }
}

// Welcome animation: "Padi" slides in, then "by Rio".
void bootSplash() {
  if (!displayReady) return;
  display.setTextColor(SSD1306_WHITE);
  for (int x = -110; x <= 16; x += 11) {
    display.clearDisplay();
    display.setTextSize(3);
    display.setCursor(x, 12);
    display.print("Padi");
    display.display();
    delay(20);
  }
  for (int w = 0; w <= 96; w += 8) {          // underline sweep
    display.drawFastHLine(16, 40, w, SSD1306_WHITE);
    display.display();
    delay(12);
  }
  display.setTextSize(1);
  display.setCursor(78, 50);
  display.print("by Rio");
  display.display();
  delay(1100);
}

// Names of currently-held notes, e.g. "C E G".
void heldNotesStr(char* buf, int bufsz) {
  bool seen[12] = {false};
  int pos = 0; buf[0] = 0;
  for (int n = 0; n < 128; n++) {
    if (!noteActive[n]) continue;
    int pc = n % 12;
    if (seen[pc]) continue;
    seen[pc] = true;
    const char* nm = NOTE_NAMES[pc];
    int l = strlen(nm);
    if (pos + l + 1 >= bufsz) break;
    if (pos > 0) buf[pos++] = ' ';
    for (int k = 0; k < l; k++) buf[pos++] = nm[k];
    buf[pos] = 0;
  }
}

// Current scale/chord/mode name (for the main screen + menus).
const char* modeName() {
  if (playStyle == STYLE_DRUMS) return "Drums";
  if (playStyle == STYLE_CHORD || playStyle == STYLE_OMNI_CHORD) return CHORD_NAMES[currentChordMode];
  if (playStyle == STYLE_CHROMATIC) return "Chromatic";
  return SCALE_NAMES[currentScale];
}

void drawMain() {
  if (!displayReady) return;
  display.clearDisplay();
  display.setTextColor(SSD1306_WHITE);

  // Header (small, size 1) so full instrument names fit; beat shown on the right
  display.setTextSize(1);
  display.setCursor(0, 0);
  display.print(CAT_NAMES[curCat]);
  display.print(' ');
  display.print(CAT_PATCH_NAMES[curCat][curPatch]);
  if (currentBeat != 0) {
    const char* bn = BEATS[currentBeat].name;
    display.setCursor(128 - (int)strlen(bn) * 6, 0);
    display.print(bn);
  }
  display.drawFastHLine(0, 10, 128, SSD1306_WHITE);

  // Hero line: scale/mode name (left, big) + note being played (right, small)
  char held[28];
  heldNotesStr(held, sizeof(held));
  display.setTextSize(2);
  display.setCursor(0, 18);
  display.print(modeName());
  if (held[0]) {
    display.setTextSize(2);
    int w = (int)strlen(held) * 12;
    display.setCursor(128 - w, 18);
    display.print(held);
  }

  // Knob-FX bar: shows which effect the knob controls + its amount (0..127)
  display.setTextSize(1);
  display.setCursor(0, 44);
  display.print(FX[knobFx].name);
  const int bx = 46, by = 43, bw = 82, bh = 11;
  display.drawRect(bx, by, bw, bh, SSD1306_WHITE);
  display.fillRect(bx + 1, by + 1, map(fxValue, 0, 127, 0, bw - 2), bh - 2, SSD1306_WHITE);

  display.display();
}

// ---------------------------------------------------------------------------
//  Note helpers (tracked, so allNotesOff() releases exactly what's sounding)
// ---------------------------------------------------------------------------
void noteOnT(byte note, byte vel) {
  note = constrain(note, 0, 127);
  MIDI.sendNoteOn(note, vel, midiChannel);
  noteActive[note] = true;
  displayDirty = true;   // show the note live
}
void noteOffT(byte note) {
  note = constrain(note, 0, 127);
  MIDI.sendNoteOff(note, 0, midiChannel);
  noteActive[note] = false;
  displayDirty = true;
}
void allNotesOff() {
  for (int n = 0; n < 128; n++) {
    if (noteActive[n]) { MIDI.sendNoteOff(n, 0, midiChannel); noteActive[n] = false; }
  }
  MIDI.sendControlChange(123, 0, midiChannel);  // All Notes Off
}

// ---------------------------------------------------------------------------
//  Volume knob: median (spike-proof) + EMA (smooth) + 1-step deadband
// ---------------------------------------------------------------------------
int readPotFiltered() {
  // Trimmed mean: oversample, sort, drop the outliers, average the middle.
  // The averaging (not median) is what tames the RP2040 ADC DNL glitch that
  // makes a still pot toggle between two codes; the trim kills touch spikes.
  const int N = 25, TRIM = 6;
  int s[N];
  for (int i = 0; i < N; i++) s[i] = analogRead(POT_PIN);
  for (int i = 1; i < N; i++) {           // insertion sort
    int k = s[i], j = i - 1;
    while (j >= 0 && s[j] > k) { s[j + 1] = s[j]; j--; }
    s[j + 1] = k;
  }
  long sum = 0;
  for (int i = TRIM; i < N - TRIM; i++) sum += s[i];
  return (int)(sum / (N - 2 * TRIM));
}
// Send the selected knob-FX at value v (0..127) as standard MIDI.
void sendFxValue(byte v) {
  if (FX[knobFx].cc == FX_PITCH) {
    int bend = map(v, 0, 127, -8192, 8191);   // 64 -> ~center
    MIDI.sendPitchBend(bend, midiChannel);
  } else {
    MIDI.sendControlChange(FX[knobFx].cc, v, midiChannel);
  }
}

void updatePot(unsigned long now) {
  static unsigned long last = 0;
  static float ema = -1;
  static int restVal = -1, lastVal = 0, stable = 0;
  static bool moving = false;
  if (now - last < POT_INTERVAL) return;
  // The OLED I2C write burst couples noise into the GP26 ADC. Don't sample the
  // pot right after a redraw, so playing notes can't nudge the FX.
  if (now - lastDisplayDraw < 30) return;
  last = now;
  int raw = readPotFiltered();
  if (ema < 0) ema = raw;
  ema += (raw - ema) * 0.15f;                              // heavy smoothing
  int r = (int)ema;

  // Fixed generous mapping to 0..127 (pot reversed; ends clamp). No auto-cal:
  // the flaky pot drifts, but the rest-latch below keeps the FX still at rest.
  int val = constrain(map(r, POT_RAW_LO, POT_RAW_HI, 127, 0), 0, 127);

  // Rest-latch: hold the FX steady unless the knob makes a REAL move well past
  // the pot's noise floor. This is what stops the "jittering all over".
  if (restVal < 0) { restVal = val; lastVal = val; }
  if (!moving) {
    if (abs(val - restVal) < 10) return;   // idle: ignore drift/jitter
    moving = true;
  }
  // Track smoothly while turning.
  const int MAXSTEP = 9;
  int d = val - (int)fxValue;
  int v = (abs(d) > MAXSTEP) ? (int)fxValue + (d > 0 ? MAXSTEP : -MAXSTEP) : val;
  if (v != (int)fxValue) {
    fxValue = v;
    sendFxValue(fxValue);          // standard MIDI CC / pitch bend
    displayDirty = true;
  }
  // Once the knob settles, latch the new rest point and go idle again.
  if (abs(val - lastVal) <= 1) stable++; else stable = 0;
  lastVal = val;
  if (stable >= 10) { moving = false; restVal = val; stable = 0; }
}

// ---------------------------------------------------------------------------
//  Drum-beat player: loops the selected groove on GM channel 10 while you play.
// ---------------------------------------------------------------------------
void beatAllOff() {
  MIDI.sendNoteOff(KICK, 0, GM_DRUM_CH);
  MIDI.sendNoteOff(SNARE, 0, GM_DRUM_CH);
  MIDI.sendNoteOff(CHAT, 0, GM_DRUM_CH);
  MIDI.sendNoteOff(OHAT, 0, GM_DRUM_CH);
}
void beatSelect(byte idx) {
  if (idx >= BEAT_COUNT) return;
  currentBeat = idx;
  beatStep = 0;
  lastStepTime = millis();
  beatAllOff();
  displayDirty = true;
}
void updateBeats(unsigned long now) {
  if (currentBeat == 0 || BEATS[currentBeat].bpm == 0) return;
  unsigned long stepMs = 60000UL / BEATS[currentBeat].bpm / 4;   // 16th notes
  if (now - lastStepTime < stepMs) return;
  lastStepTime = now;
  const Beat& b = BEATS[currentBeat];
  uint16_t bit = (uint16_t)1 << beatStep;
  beatAllOff();                                  // retrigger cleanly
  if (b.kick  & bit) MIDI.sendNoteOn(KICK,  110, GM_DRUM_CH);
  if (b.snare & bit) MIDI.sendNoteOn(SNARE, 105, GM_DRUM_CH);
  if (b.chat  & bit) MIDI.sendNoteOn(CHAT,   80, GM_DRUM_CH);
  if (b.ohat  & bit) MIDI.sendNoteOn(OHAT,   90, GM_DRUM_CH);
  beatStep = (beatStep + 1) % 16;
}

// ---------------------------------------------------------------------------
//  Instrument selection (sends Program Change; PC bridge maps it to a Surge
//  patch). Plays a short 3-note preview so you hear what you picked.
// ---------------------------------------------------------------------------
void selectInstrument(byte cat, byte patch) {
  curCat = cat;
  curPatch = patch;
  sendInstrument(cat, patch);
  playStyle = STYLE_SCALED;
  displayDirty = true;
  drawMenuNow();                 // refresh selection marker before the preview delay
  delay(180);                    // let the patch load (bridge -> Surge)
  const byte prev[3] = {60, 64, 67};   // C E G
  for (byte i = 0; i < 3; i++) {
    noteOnT(prev[i], 105);
    delay(170);
    noteOffT(prev[i]);
    delay(30);
  }
  writeFlash();   // remember this instrument across reboots/reconnects
}

// ---------------------------------------------------------------------------
//  Knob-FX selection: choose which effect the knob controls. Resets the old
//  effect to neutral, then previews the new one on the current instrument by
//  holding a note and sweeping the effect so you hear what it does.
// ---------------------------------------------------------------------------
void selectFx(byte idx) {
  sendFxValue(FX[knobFx].neutral);   // clear the OLD effect so it doesn't linger
  knobFx = idx;
  fxValue = FX[idx].neutral;
  displayDirty = true;
  drawMenuNow();
  // Preview: hold a note and sweep the effect up then back to neutral.
  noteOnT(60, 105);
  byte lo = FX[idx].neutral, hi = (lo <= 64) ? 127 : 0;   // sweep toward the "on" extreme
  for (int step = 0; step <= 20; step++) {
    byte v = map(step, 0, 20, lo, hi);
    sendFxValue(v);
    delay(45);
  }
  delay(120);
  for (int step = 0; step <= 12; step++) {                // ease back to neutral
    byte v = map(step, 0, 12, hi, lo);
    sendFxValue(v);
    delay(25);
  }
  noteOffT(60);
  fxValue = FX[idx].neutral;
  writeFlash();   // remember the knob-FX choice across reboots
}

// ---------------------------------------------------------------------------
//  On-screen menu.  Pages: 1 Presets  2 Scales  3 Chords  4 Instruments
//  Top row: 13=<prev page  14=next page>  15/16=EXIT.  Selection marked with >.
// ---------------------------------------------------------------------------
void drawMenuItem(byte slot, byte number, const char* label, bool sel) {
  int x = (slot % 2) * 64;
  int y = 12 + (slot / 2) * 12;   // rows at 12,24,36,48
  display.setCursor(x, y);
  display.print(sel ? '>' : ' ');
  display.print(number);
  display.print(' ');
  for (byte c = 0; c < 8 && label[c]; c++) display.print(label[c]);
}

void drawMenu() {
  if (!displayReady) return;
  display.clearDisplay();
  display.setTextColor(SSD1306_WHITE);
  display.setTextSize(1);

  const char* footer = "13<4> pg  15/16 exit";
  if (currentPage == 1) {          // INSTRUMENTS (Surge catalog categories)
    display.setCursor(0, 0); display.print(CAT_NAMES[instCat]);
    display.setCursor(102, 0); display.print(instCat + 1); display.print("/"); display.print(CAT_COUNT);
    for (byte i = 0; i < CAT_SIZE; i++) {
      bool sel = (instCat == curCat && i == curPatch);
      drawMenuItem(i, i + 1, CAT_PATCH_NAMES[instCat][i], sel);
    }
    footer = "1-8 pick  9/10 categ";
  } else if (currentPage == 2) {   // BEATS
    display.setCursor(0, 0); display.print("BEATS");
    for (byte i = 0; i < BEAT_COUNT; i++) drawMenuItem(i, i + 1, BEATS[i].name, i == currentBeat);
    footer = "1-N beat  13<4>  16 exit";
  } else if (currentPage == 3) {   // SCALES (+ Drums mode on pad 9)
    display.setCursor(0, 0); display.print(playStyle == STYLE_DRUMS ? "SCALES  *DRUMS*" : "SCALES");
    for (byte i = 0; i < 8; i++)
      drawMenuItem(i, i + 1, SCALE_NAMES[i], playStyle != STYLE_DRUMS && i == currentScale);
    footer = "1-8 scale  9 Drums";
  } else if (currentPage == 4) {   // CHORDS
    display.setCursor(0, 0); display.print("CHORDS");
    for (byte i = 0; i < 8; i++) drawMenuItem(i, i + 1, CHORD_NAMES[i], i == currentChordMode);
    footer = "1-8 pick  13<4>  16 exit";
  } else {                          // KNOB FX (what the knob controls)
    display.setCursor(0, 0); display.print("KNOB FX");
    for (byte i = 0; i < 8; i++) drawMenuItem(i, i + 1, FX[i].name, i == knobFx);
    footer = "1-8 pick  13<4>  16 exit";
  }
  display.setCursor(0, 56);
  display.print(footer);
  display.display();
}

// Force an immediate menu redraw (used mid-selectInstrument, before its delay).
void drawMenuNow() {
  lastDisplayDraw = millis();
  displayDirty = false;
  drawMenu();
}

void setup() {
  usb_midi.setStringDescriptor("Macro MIDI Pad");
  MIDI.begin(MIDI_CHANNEL_OMNI);

  pinMode(LED_PIN, OUTPUT);
  for (int i = 0; i < 16; i++) {
    pinMode(PAD_PINS[i], INPUT_PULLUP);
  }

  initDisplay();
  bootSplash();

  // Load presets from flash
  loadPresetsFromFlash();
  
  // Load first preset
  loadPreset(0);
  
  // Flash LED
  for (int i = 0; i < 3; i++) {
    digitalWrite(LED_PIN, HIGH);
    delay(100);
    digitalWrite(LED_PIN, LOW);
    delay(100);
  }

  // curCat/curPatch/knobFx were restored from flash in loadPresetsFromFlash
  // (defaults only if none saved yet). Open the menu on the current category.
  instCat = curCat;
  fxValue = FX[knobFx].neutral;           // start the knob-FX at neutral
  programAnnounceAt = millis() + 4000;   // tell the host once it's listening
  displayDirty = true;
}

void loop() {
  unsigned long now = millis();
  
  if (playStyle == STYLE_GENERATIVE && !inMenu) {
    updateGenerative(now);
  }
  
  for (int i = 0; i < 16; i++) {
    bool reading = (digitalRead(PAD_PINS[i]) == LOW);
    
    if (reading != lastPadStates[i]) {
      lastDebounceTime[i] = now;
    }
    
    if ((now - lastDebounceTime[i]) > DEBOUNCE_DELAY) {
      if (reading != padStates[i]) {
        padStates[i] = reading;
        
        if (padStates[i]) {
          padHoldStartTime[i] = now;
          handlePadPress(i, now);
        } else {
          handlePadRelease(i, now);
          padHoldStartTime[i] = 0;
        }
      }
    }
    
    lastPadStates[i] = reading;
  }
  
  // Check for preset save
  if (inMenu && currentPage == 1) {
    for (int i = 0; i < 4; i++) {
      if (padStates[i] && padHoldStartTime[i] > 0) {
        unsigned long holdDuration = now - padHoldStartTime[i];
        if (holdDuration > PRESET_SAVE_HOLD_TIME && holdDuration < PRESET_SAVE_HOLD_TIME + 100) {
          savePreset(i);
        }
      }
    }
  }
  
  updatePot(now);
  updateBeats(now);

  // Redraw the screen when something changed (throttled; slower = less ADC noise).
  if (displayDirty && now - lastDisplayDraw > 70) {
    displayDirty = false;
    lastDisplayDraw = now;
    if (inMenu) drawMenu();
    else        drawMain();
  }

  // Announce the current instrument shortly after boot (host/bridge not ready in setup()).
  if (programAnnounceAt && now >= programAnnounceAt) {
    programAnnounceAt = 0;
    sendInstrument(curCat, curPatch);
  }

  MIDI.read();
}

void handlePadPress(byte pad, unsigned long now) {
  for (int i = 0; i < 4; i++) {
    if (pad == TOP_ROW[i]) {
      topRowPressTime[i] = now;
      break;
    }
  }
  
  if (padStates[TOP_ROW[0]] && padStates[TOP_ROW[1]] && 
      padStates[TOP_ROW[2]] && padStates[TOP_ROW[3]]) {
    
    unsigned long minT = topRowPressTime[0], maxT = topRowPressTime[0];
    for (int i = 1; i < 4; i++) {
      if (topRowPressTime[i] < minT) minT = topRowPressTime[i];
      if (topRowPressTime[i] > maxT) maxT = topRowPressTime[i];
    }
    
    if ((maxT - minT) < 150) {
      toggleMenu();
      topRowPressed = true;
      return;
    }
  }
  
  if (inMenu) return;
  
  playNote(pad, true);
}

void handlePadRelease(byte pad, unsigned long now) {
  if (topRowPressed) {
    bool isTopRow = false;
    for (int i = 0; i < 4; i++) {
      if (pad == TOP_ROW[i]) {
        isTopRow = true;
        break;
      }
    }
    if (isTopRow) {
      bool allReleased = true;
      for (int i = 0; i < 4; i++) {
        if (padStates[TOP_ROW[i]]) {
          allReleased = false;
          break;
        }
      }
      if (allReleased) topRowPressed = false;
      return;
    }
  }
  
  if (inMenu) {
    handleMenuPress(pad);
  } else {
    playNote(pad, false);
  }
}

void handleMenuPress(byte pad) {
  // Top row is the same on every page: 13=<prev  14=next>  15/16=EXIT.
  if (pad == 12) { currentPage = (currentPage == 1) ? 5 : currentPage - 1; displayDirty = true; return; }
  if (pad == 13) { currentPage = (currentPage == 5) ? 1 : currentPage + 1; displayDirty = true; return; }
  if (pad == 14 || pad == 15) { toggleMenu(); return; }  // exit from any page

  if (currentPage == 1) {          // INSTRUMENTS: 1-8 pick, 9/10 change category
    if (pad < CAT_SIZE) selectInstrument(instCat, pad);
    else if (pad == 8) instCat = (instCat + CAT_COUNT - 1) % CAT_COUNT;  // pad 9 = prev category
    else if (pad == 9) instCat = (instCat + 1) % CAT_COUNT;              // pad 10 = next category
  } else if (currentPage == 2) {   // BEATS
    if (pad < BEAT_COUNT) beatSelect(pad);
  } else if (currentPage == 3) {   // SCALES
    if (pad < 8) { currentScale = pad; playStyle = STYLE_SCALED; }
    else if (pad == 8) playStyle = STYLE_DRUMS;   // pad 9 = GM drum-kit mode
  } else if (currentPage == 4) {   // CHORDS
    if (pad < 8) { currentChordMode = pad; currentScale = CHORD_MODE_SCALES[pad]; playStyle = STYLE_CHORD; }
  } else if (currentPage == 5) {   // KNOB FX
    if (pad < 8) selectFx(pad);
  }

  displayDirty = true;
}

void playNote(byte pad, bool noteOn) {
  if (playStyle == STYLE_GENERATIVE) return;
  
  switch(playStyle) {
    case STYLE_SCALED:
      {
        byte note = calcScaledNote(pad);
        if (noteOn) noteOnT(note, velocity);
        else noteOffT(note);
      }
      break;
      
    case STYLE_CHROMATIC:
      {
        byte note = rootNote + pad + transpose + (octave * 12);
        note = constrain(note, 0, 127);
        if (noteOn) noteOnT(note, velocity);
        else noteOffT(note);
      }
      break;
      
    case STYLE_CHORD:
      playChord(pad, noteOn);
      break;
      
    case STYLE_OMNI_CHORD:
      playOmniChord(pad, noteOn);
      break;

    case STYLE_DRUMS:
      {
        byte note = DRUM_NOTES[pad];   // each pad = a GM drum sound
        if (noteOn) MIDI.sendNoteOn(note, velocity, GM_DRUM_CH);
        else        MIDI.sendNoteOff(note, 0, GM_DRUM_CH);
      }
      break;
  }
}

byte calcScaledNote(byte pad) {
  // Linear scales - directly use pad index (0-15) into the 16-note scale array
  int note = rootNote + SCALES[currentScale][pad] + transpose + (octave * 12);
  return constrain(note, 0, 127);
}

void playChord(byte pad, bool noteOn) {
  if (pad < 8) {
    // Bottom 8 pads: Play chords (1-5 notes depending on voicing)
    for (int i = 0; i < 5; i++) {
      int8_t interval = CHORD_VOICINGS[currentChordMode][pad][i];
      
      // Skip if this is a NO_NOTE marker
      if (interval == NO_NOTE) continue;
      
      int note = rootNote + interval + transpose + (octave * 12);
      note = constrain(note, 0, 127);
      if (noteOn) noteOnT(note, velocity);
      else noteOffT(note);
    }
  } else {
    // Top 8 pads: Play single notes from the chord mode's scale
    byte noteIndex = pad - 8;
    byte chordScale = CHORD_MODE_SCALES[currentChordMode];
    int note = rootNote + 12 + SCALES[chordScale][noteIndex] + transpose + (octave * 12);
    note = constrain(note, 0, 127);
    if (noteOn) noteOnT(note, velocity);
    else noteOffT(note);
  }
}

void playOmniChord(byte pad, bool noteOn) {
  if (pad < 8) {
    // Bottom 8 pads: Select chord (like fretting)
    if (noteOn) {
      omniChordIndex = pad;
    }
  } else {
    // Top 8 pads: Pluck individual notes from the selected chord
    byte stringIndex = pad - 8;  // Which "string" (note in chord) to pluck
    
    if (noteOn) {
      // All 8 top pads can now play notes from the chord
      int8_t interval = OMNI_CHORD_VOICINGS[omniChordIndex][stringIndex];  // Use 8-note omni chord voicings
      
      int note = rootNote + interval + transpose + (octave * 12);
      note = constrain(note, 0, 127);
      
      // Store the actual MIDI note for this pad
      omniChordNotes[stringIndex] = note;
      noteOnT(note, velocity);
    } else {
      // Note off: use the stored note for this pad
      if (omniChordNotes[stringIndex] != 255) {
        noteOffT(omniChordNotes[stringIndex]);
        omniChordNotes[stringIndex] = 255;  // Clear the stored note
      }
    }
  }
}

void updateGenerative(unsigned long now) {
  if (now - lastGenNoteTime >= nextGenNoteDelay) {
    
    if (currentGenNote != 255) {
      noteOffT(currentGenNote);
      currentGenNote = 255;
      delay(10);
    }
    
    if (random(0, 100) < GEN_TRIGGER_CHANCE) {
      byte octaveOffset = random(0, 3) * 12;
      byte noteIndex = random(0, 8);
      
      // Use scale 0 (Major) for generative mode instead of user's scale
      int note = rootNote + octaveOffset + SCALES[0][noteIndex] + transpose + (octave * 12);
      currentGenNote = constrain(note, 0, 127);
      
      noteOnT(currentGenNote, random(60, 90));
    }
    
    nextGenNoteDelay = random(MIN_GEN_DELAY, MAX_GEN_DELAY);
    lastGenNoteTime = now;
  }
}

void toggleMenu() {
  inMenu = !inMenu;
  digitalWrite(LED_PIN, inMenu ? HIGH : LOW);
  allNotesOff();          // release only what's sounding (no 128-msg blast)
  displayDirty = true;
  if (inMenu) currentPage = 1;
}

void loadPreset(byte index) {
  playStyle = presets[index].style;
  currentScale = presets[index].scale;
  currentChordMode = presets[index].chordMode;
  transpose = presets[index].transpose;
  octave = presets[index].octave;
  velocity = presets[index].velocity;
}

void savePreset(byte index) {
  presets[index] = {playStyle, currentScale, currentChordMode, transpose, octave, velocity};
  savePresetsToFlash();  // Write to flash
}

void resetToDefaults() {
  playStyle = STYLE_SCALED;
  currentScale = 0;
  currentChordMode = 0;
  transpose = 0;
  octave = -1;
  velocity = 100;
  midiChannel = 1;
  
  // Reset all presets to defaults
  presets[0] = {STYLE_SCALED, 0, 0, 0, -1, 100};
  presets[1] = {STYLE_CHROMATIC, 7, 0, 0, -1, 100};
  presets[2] = {STYLE_CHORD, 0, 0, 0, -1, 100};
  presets[3] = {STYLE_GENERATIVE, 0, 0, 0, -1, 80};
  
  savePresetsToFlash();  // Write to flash
}