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

// ---- Volume knob (pot on GP26): median + EMA + deadband -> 1..100, CC7 ----
#define POT_PIN 26
const byte VOLUME_CC = 7;
byte volume = 100;                 // 1..100
const int POT_RAW_MIN = 200;       // calibration (pot wired reversed)
const int POT_RAW_MAX = 900;
const unsigned long POT_INTERVAL = 15;

// ---- Note tracking (so we release exactly what's sounding; no 128-msg blast) ----
bool noteActive[128] = {false};

const char* PRESET_NAMES[4] = {"Major", "Chromatic", "Chords", "Generat"};

// ---- Current instrument (GM program) + which family the menu is browsing ----
byte currentProgram = 0;   // 0 = Grand Piano
byte instFamily = 0;       // 0..15 (Piano, Chromatic Perc, Organ, Guitar, ...)
unsigned long programAnnounceAt = 0;  // resend Program Change once host is up

const char* NOTE_NAMES[12] = {"C","C#","D","D#","E","F","F#","G","G#","A","A#","B"};

// ---- Drum-beat presets: looping grooves you play instruments over ----
// 16-step patterns (bit0 = step 0). GM drums on channel 10.
struct Beat { const char* name; byte bpm; uint16_t kick, snare, chat, ohat; };
const Beat BEATS[] = {
  {"Off",     0,  0x0000, 0x0000, 0x0000, 0x0000},
  {"RnB",     86, 0x0441, 0x1010, 0x5555, 0x0000},
  {"HipHop",  90, 0x0481, 0x1010, 0x5555, 0x0000},
  {"Trap",    70, 0x0041, 0x0100, 0xFFFF, 0x0000},
  {"House",  124, 0x1111, 0x1010, 0x0000, 0x4444},
  {"Pop",    100, 0x0101, 0x1010, 0x5555, 0x0000},
  {"Boom",    92, 0x0421, 0x1010, 0x1111, 0x0000},
};
const byte BEAT_COUNT = sizeof(BEATS) / sizeof(BEATS[0]);
byte currentBeat = 0;              // 0 = off
byte beatStep = 0;
unsigned long lastStepTime = 0;
int bpm = 90;                      // global tempo (drives beats + synced FX)
const byte GM_DRUM_CH = 10;
const byte KICK = 36, SNARE = 38, CHAT = 42, OHAT = 46;

enum PlayStyle {STYLE_SCALED, STYLE_CHROMATIC, STYLE_CHORD, STYLE_GENERATIVE, STYLE_OMNI_CHORD};

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
#define FLASH_MAGIC 0x4D494449  // "MIDI" in hex
struct FlashData {
  uint32_t magic;
  Preset presets[4];
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
  } else {
    // No valid data, use defaults
    presets[0] = {STYLE_SCALED, 0, 0, 0, -1, 100};
    presets[1] = {STYLE_CHROMATIC, 7, 0, 0, -1, 100};  // Chromatic scale
    presets[2] = {STYLE_CHORD, 0, 0, 0, -1, 100};
    presets[3] = {STYLE_GENERATIVE, 0, 0, 0, -1, 80};
  }
}

void savePresetsToFlash() {
  FlashData data;
  data.magic = FLASH_MAGIC;
  for (int i = 0; i < 4; i++) {
    data.presets[i] = presets[i];
  }
  
  uint8_t buffer[FLASH_PAGE_SIZE];
  memcpy(buffer, &data, sizeof(FlashData));
  
  uint32_t ints = save_and_disable_interrupts();
  flash_range_erase(FLASH_TARGET_OFFSET, FLASH_SECTOR_SIZE);
  flash_range_program(FLASH_TARGET_OFFSET, buffer, FLASH_PAGE_SIZE);
  restore_interrupts(ints);
  
  // Flash LED to confirm
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
  display.print(GM_NAMES[currentProgram]);
  if (currentBeat != 0) {
    char b[16];
    snprintf(b, sizeof(b), "%s %d", BEATS[currentBeat].name, bpm);
    display.setCursor(128 - (int)strlen(b) * 6, 0);
    display.print(b);
  }
  display.drawFastHLine(0, 10, 128, SSD1306_WHITE);

  // Hero line: scale/mode name (left, big) + note being played (right, small)
  char held[28];
  heldNotesStr(held, sizeof(held));
  display.setTextSize(2);
  display.setCursor(0, 18);
  display.print(modeName());
  if (held[0]) {
    display.setTextSize(1);
    int w = (int)strlen(held) * 6;
    display.setCursor(128 - w, 22);
    display.print(held);
  }

  // Volume bar 0..100
  display.setTextSize(1);
  display.setCursor(0, 44);
  display.print("Vol");
  const int bx = 24, by = 43, bw = 74, bh = 11;
  display.drawRect(bx, by, bw, bh, SSD1306_WHITE);
  display.fillRect(bx + 1, by + 1, map(volume, 0, 100, 0, bw - 2), bh - 2, SSD1306_WHITE);
  display.setCursor(bx + bw + 5, by + 2);
  display.print(volume);

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
int readPotMedian() {
  const int N = 9;
  int s[N];
  for (int i = 0; i < N; i++) s[i] = analogRead(POT_PIN);
  for (int i = 1; i < N; i++) {           // insertion sort
    int k = s[i], j = i - 1;
    while (j >= 0 && s[j] > k) { s[j + 1] = s[j]; j--; }
    s[j + 1] = k;
  }
  return s[N / 2];
}
void updatePot(unsigned long now) {
  static unsigned long last = 0;
  static float ema = -1;
  if (now - last < POT_INTERVAL) return;
  last = now;
  int raw = readPotMedian();
  if (ema < 0) ema = raw;
  ema += (raw - ema) * 0.25f;
  int v = constrain(map((int)ema, POT_RAW_MIN, POT_RAW_MAX, 100, 0), 0, 100);  // reversed
  // Ignore tiny +/-1-2 jitter, but always allow snapping to the exact ends.
  bool bigEnough = abs(v - (int)volume) >= 3;
  bool hitEnd = (v == 0 || v == 100) && v != volume;
  if (bigEnough || hitEnd) {
    volume = v;
    int cc = map(volume, 0, 100, 0, 127);
    MIDI.sendControlChange(VOLUME_CC, cc, midiChannel);   // melodic (Surge)
    MIDI.sendControlChange(VOLUME_CC, cc, GM_DRUM_CH);    // drums (GM kit)
    displayDirty = true;
  }
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
  if (idx > 0 && BEATS[idx].bpm > 0) bpm = BEATS[idx].bpm;  // adopt the groove's tempo
  beatStep = 0;
  lastStepTime = millis();
  beatAllOff();
  displayDirty = true;
}

// Tap tempo: tap in time; averages the last few intervals into BPM.
void tapTempo() {
  static unsigned long prev = 0;
  static unsigned long intervals[3];
  static byte n = 0;
  unsigned long now = millis();
  if (prev != 0 && now - prev < 2000) {          // within a sensible window
    intervals[n % 3] = now - prev;
    n++;
    byte cnt = n < 3 ? n : 3;
    unsigned long sum = 0;
    for (byte i = 0; i < cnt; i++) sum += intervals[i];
    unsigned long avg = sum / cnt;
    if (avg > 0) bpm = constrain((int)(60000UL / avg), 40, 220);
  } else {
    n = 0;                                        // first tap / restart
  }
  prev = now;
  displayDirty = true;
}
void adjustBpm(int d) { bpm = constrain(bpm + d, 40, 220); displayDirty = true; }

void updateBeats(unsigned long now) {
  if (currentBeat == 0) return;
  unsigned long stepMs = 60000UL / bpm / 4;   // 16th notes at the global tempo
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
void selectInstrument(byte prog) {
  currentProgram = prog;
  MIDI.sendProgramChange(prog, midiChannel);
  playStyle = STYLE_SCALED;
  displayDirty = true;
  drawMenuNow();                 // refresh selection marker before the preview delay
  delay(180);                    // let the patch load (bridge -> Surge)
  // Loading a patch resets Surge's volume to full, so re-apply the knob level.
  MIDI.sendControlChange(VOLUME_CC, map(volume, 0, 100, 0, 127), midiChannel);
  const byte prev[3] = {60, 64, 67};   // C E G
  for (byte i = 0; i < 3; i++) {
    noteOnT(prev[i], 105);
    delay(170);
    noteOffT(prev[i]);
    delay(30);
  }
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
  if (currentPage == 1) {          // INSTRUMENTS (family = category)
    display.setCursor(0, 0); display.print(GM_FAMILY_NAMES[instFamily]);
    display.setCursor(96, 0); display.print(instFamily + 1); display.print("/16");
    for (byte i = 0; i < 8; i++) {
      byte prog = instFamily * 8 + i;
      drawMenuItem(i, i + 1, GM_NAMES[prog], prog == currentProgram);
    }
    footer = "1-8 pick  9/10 categ";
  } else if (currentPage == 2) {   // BEATS + tempo
    display.setCursor(0, 0); display.print("BEATS  ");
    display.print(bpm); display.print(" bpm");
    for (byte i = 0; i < BEAT_COUNT; i++) drawMenuItem(i, i + 1, BEATS[i].name, i == currentBeat);
    footer = "9 tap  10/11 bpm  16 exit";
  } else if (currentPage == 3) {   // SCALES
    display.setCursor(0, 0); display.print("SCALES");
    for (byte i = 0; i < 8; i++) drawMenuItem(i, i + 1, SCALE_NAMES[i], i == currentScale);
    footer = "1-8 pick  13<4>  16 exit";
  } else {                          // CHORDS
    display.setCursor(0, 0); display.print("CHORDS");
    for (byte i = 0; i < 8; i++) drawMenuItem(i, i + 1, CHORD_NAMES[i], i == currentChordMode);
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

  currentProgram = 0;                    // default: Grand Piano
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
  
  updatePot(now);
  updateBeats(now);
  updateFX(now);

  // Redraw the screen when something changed (throttled).
  if (displayDirty && now - lastDisplayDraw > 40) {
    displayDirty = false;
    lastDisplayDraw = now;
    if (inMenu) drawMenu();
    else        drawMain();
  }

  // Announce the current instrument shortly after boot (host/bridge not ready in setup()).
  if (programAnnounceAt && now >= programAnnounceAt) {
    programAnnounceAt = 0;
    MIDI.sendProgramChange(currentProgram, midiChannel);
  }

  MIDI.read();
}

// ---------------------------------------------------------------------------
//  Hold-to-activate performance FX on the top row (GP12-15):
//  12 Filter (muffle) | 13 Tape-stop | 14 Echo throw | 15 Stutter (BPM gate)
// ---------------------------------------------------------------------------
const byte FILT_CC = 74, ECHO_CC = 93;
bool fxTapeStop = false;  unsigned long fxTapeStart = 0, fxLastPb = 0;
bool fxStutter = false;   unsigned long fxLastGate = 0; bool fxGateMuted = false;

bool isTopRow(byte pad) { return pad >= 12 && pad <= 15; }

void fxOn(byte pad) {
  if (pad == 12) MIDI.sendControlChange(FILT_CC, 16, midiChannel);          // close filter
  else if (pad == 13) { fxTapeStop = true; fxTapeStart = millis(); }        // tape-stop ramp
  else if (pad == 14) MIDI.sendControlChange(ECHO_CC, 110, midiChannel);    // echo send up
  else if (pad == 15) { fxStutter = true; fxLastGate = millis(); fxGateMuted = false; }
}
void fxOff(byte pad) {
  int vcc = map(volume, 0, 100, 0, 127);
  if (pad == 12) MIDI.sendControlChange(FILT_CC, 127, midiChannel);         // open filter
  else if (pad == 13) { fxTapeStop = false; MIDI.sendPitchBend(0, midiChannel); }
  else if (pad == 14) MIDI.sendControlChange(ECHO_CC, 0, midiChannel);      // echo off
  else if (pad == 15) { fxStutter = false; MIDI.sendControlChange(7, vcc, midiChannel); MIDI.sendControlChange(7, vcc, GM_DRUM_CH); }
}
void clearFX() {                         // reset everything (e.g. when menu opens)
  fxTapeStop = fxStutter = false;
  int vcc = map(volume, 0, 100, 0, 127);
  MIDI.sendPitchBend(0, midiChannel);
  MIDI.sendControlChange(FILT_CC, 127, midiChannel);
  MIDI.sendControlChange(ECHO_CC, 0, midiChannel);
  MIDI.sendControlChange(7, vcc, midiChannel);
  MIDI.sendControlChange(7, vcc, GM_DRUM_CH);
}
void updateFX(unsigned long now) {
  if (fxTapeStop && now - fxLastPb >= 20) {          // ramp pitch down over ~600ms
    fxLastPb = now;
    long bend = -(long)constrain(((now - fxTapeStart) * 8192UL) / 600UL, 0UL, 8192UL);
    MIDI.sendPitchBend((int)bend, midiChannel);
  }
  if (fxStutter) {                                    // gate the mix on/off at 1/16
    unsigned long gate = 60000UL / bpm / 4;
    if (now - fxLastGate >= gate) {
      fxLastGate = now;
      fxGateMuted = !fxGateMuted;
      int lvl = fxGateMuted ? 0 : map(volume, 0, 100, 0, 127);
      MIDI.sendControlChange(7, lvl, midiChannel);
      MIDI.sendControlChange(7, lvl, GM_DRUM_CH);
    }
  }
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
  if (isTopRow(pad)) { fxOn(pad); return; }   // top row = hold-to-activate FX

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
  } else if (isTopRow(pad)) {
    fxOff(pad);
  } else {
    playNote(pad, false);
  }
}

void handleMenuPress(byte pad) {
  // Top row is the same on every page: 13=<prev  14=next>  15/16=EXIT.
  if (pad == 12) { currentPage = (currentPage == 1) ? 4 : currentPage - 1; displayDirty = true; return; }
  if (pad == 13) { currentPage = (currentPage == 4) ? 1 : currentPage + 1; displayDirty = true; return; }
  if (pad == 14 || pad == 15) { toggleMenu(); return; }  // exit from any page

  if (currentPage == 1) {          // INSTRUMENTS: 1-8 pick, 9/10 change category
    if (pad < 8) selectInstrument(instFamily * 8 + pad);
    else if (pad == 8) instFamily = (instFamily + 15) % 16;   // pad 9 = prev category
    else if (pad == 9) instFamily = (instFamily + 1) % 16;    // pad 10 = next category
  } else if (currentPage == 2) {   // BEATS + tempo
    if (pad < BEAT_COUNT) beatSelect(pad);
    else if (pad == 8) tapTempo();      // pad 9  = TAP
    else if (pad == 9) adjustBpm(-5);   // pad 10 = BPM -
    else if (pad == 10) adjustBpm(5);   // pad 11 = BPM +
  } else if (currentPage == 3) {   // SCALES
    if (pad < 8) { currentScale = pad; playStyle = STYLE_SCALED; }
  } else if (currentPage == 4) {   // CHORDS
    if (pad < 8) { currentChordMode = pad; currentScale = CHORD_MODE_SCALES[pad]; playStyle = STYLE_CHORD; }
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
  clearFX();              // cancel any held FX from the open-combo
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