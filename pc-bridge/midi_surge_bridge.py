"""
Macropad -> Surge bridge (Windows "power-mode").

The macropad is a generic GM controller: it sends standard MIDI Program Change
(0-127) to pick an instrument. Surge XT is NOT a GM synth, so this bridge maps
each GM program to a hand-picked Surge patch and tells the headless surge-xt-cli
to load it via OSC (/patch/load).

Notes go straight from the Pico to surge-xt-cli (it is bound to the Pico with
--all-midi-inputs); this bridge only translates Program Change -> patch.

Run (from the midienv venv):
    python midi_surge_bridge.py            # normal: listen to the Pico
    python midi_surge_bridge.py --test     # demo: cycle a few GM instruments

Requires: mido, python-rtmidi, python-osc.  surge-xt-cli must be running with
    --osc-in-port=53280
"""
import sys, time, os, datetime
import mido
from pythonosc.udp_client import SimpleUDPClient

OSC_HOST, OSC_PORT = "127.0.0.1", 53280
MIDI_PORT_MATCH = "Macro MIDI Pad"
DRUM_OUT_MATCH = "Microsoft GS Wavetable Synth"  # Windows built-in GM synth (real drum kit)
DRUM_CHANNEL = 9  # MIDI channel 10, 0-indexed (GM percussion)
BRIDGE_LOG = os.path.join(os.path.dirname(os.path.abspath(__file__)), "bridge.log")


def blog(msg):
    line = f"{datetime.datetime.now():%H:%M:%S}  {msg}"
    print(line, flush=True)
    try:
        with open(BRIDGE_LOG, "a", encoding="utf-8") as f:
            f.write(line + "\n")
    except OSError:
        pass

BASE = r"C:\Users\rio18\Tools\SurgeXT\Surge Synth Team\SurgeXTData"
AZ = BASE + r"\patches_3rdparty\Aleksey Zhehanov"   # realistic acoustic GM set
F  = BASE + r"\patches_factory"                       # synths / keys / pads

# One representative patch per GM family (fallback for any unmapped program).
FAMILY_DEFAULT = {
    0:  AZ + r"\Keys\Grand Piano",
    1:  AZ + r"\Bells\Chime Bell",
    2:  F  + r"\Keys\Organ 1",
    3:  AZ + r"\Guitars\Acoustic Guitar",
    4:  AZ + r"\Basses\Electric Bass",
    5:  AZ + r"\Strings\String Section",
    6:  AZ + r"\Strings\String Section",
    7:  AZ + r"\Brass\Trumpet",
    8:  AZ + r"\Winds\Alto Sax",
    9:  AZ + r"\Winds\Lyrical Flute",
    10: F  + r"\Leads\Square",
    11: F  + r"\Pads\Pad 1",
    12: F  + r"\Pads\Distant",
    13: AZ + r"\Guitars\Acoustic Guitar",
    14: AZ + r"\Drums\Tom",
    15: F  + r"\Pads\Distant",
}

# Specific GM program -> patch overrides (better matches than the family default).
OVERRIDES = {
    0:  AZ + r"\Keys\Grand Piano",
    1:  AZ + r"\Keys\Upright Piano",
    2:  AZ + r"\Keys\Soft Piano",
    4:  F  + r"\Keys\EP 1",
    5:  F  + r"\Keys\DX EP",
    6:  F  + r"\Keys\Digi Harpsi",
    16: F  + r"\Keys\Organ 1",
    17: F  + r"\Keys\Organ 2",
    18: F  + r"\Keys\Organ 3",
    19: F  + r"\Keys\Church",
    20: F  + r"\Keys\House Organ",
    21: AZ + r"\Keys\Accordion Lead",
    24: AZ + r"\Guitars\Acoustic Guitar",
    25: AZ + r"\Guitars\Steel String Guitar",
    26: AZ + r"\Guitars\Electric Guitar",
    27: AZ + r"\Guitars\Electric Guitar",
    28: AZ + r"\Guitars\Muted Electric Guitar",
    29: AZ + r"\Guitars\Distortion Velo",
    30: AZ + r"\Guitars\Distortion Velo",
    31: AZ + r"\Guitars\Bright Steel String",
    32: AZ + r"\Basses\Double Bass",
    33: AZ + r"\Basses\Electric Bass",
    34: AZ + r"\Basses\Electric Bass",
    35: AZ + r"\Basses\Double Bass",
    38: F  + r"\Leads\Tight Bassline",
    39: F  + r"\Leads\Quick Basic",
    40: F  + r"\Leads\Violini Solo",
    41: AZ + r"\Strings\String Section",
    42: AZ + r"\Strings\String Section",
    43: AZ + r"\Basses\Double Bass",
    44: AZ + r"\Strings\Lo-Fi Strings",
    45: AZ + r"\Strings\Pizzicato Strings",
    46: AZ + r"\Strings\Harp",
    48: AZ + r"\Strings\String Section",
    49: AZ + r"\Strings\Lo-Fi Strings",
    50: F  + r"\Pads\Subtle Comb Strings",
    51: F  + r"\Pads\Stretch",
    52: F  + r"\Pads\Retro Choir",
    53: F  + r"\Pads\Ooh",
    54: F  + r"\Pads\Synth Choir MW O-Ah",
    56: AZ + r"\Brass\Trumpet",
    57: AZ + r"\Brass\Trombone",
    58: AZ + r"\Brass\Tuba",
    59: AZ + r"\Brass\Flugelhorn",
    60: AZ + r"\Brass\French Horn",
    61: AZ + r"\Brass\Trumpet Ensemble",
    62: AZ + r"\Brass\Trombone Ensemble",
    63: AZ + r"\Brass\Euphonium Ensemble",
    64: AZ + r"\Winds\Soprano Sax",
    65: AZ + r"\Winds\Alto Sax",
    66: AZ + r"\Winds\Alto Sax",
    67: AZ + r"\Winds\Alto Sax",
    70: AZ + r"\Winds\Subtractive Flute",
    72: AZ + r"\Winds\Lyrical Flute",
    73: AZ + r"\Winds\Lyrical Flute",
    74: AZ + r"\Winds\Subtractive Flute",
    75: AZ + r"\Winds\Subtractive Flute",
    80: F  + r"\Leads\Square",
    81: F  + r"\Leads\Moogy Saw",
    82: F  + r"\Leads\Sine Lead",
    83: F  + r"\Leads\Crisp PWM",
    84: F  + r"\Leads\Scream Lead",
    85: F  + r"\Leads\Vocal Lead",
    86: F  + r"\Leads\Saw Octaves",
    87: F  + r"\Leads\Turbo Solo",
    88: F  + r"\Pads\Pad 1",
    89: F  + r"\Pads\MKS-70 Warm Pad",
    90: F  + r"\Pads\Super",
    91: F  + r"\Pads\Retro Choir",
    92: F  + r"\Pads\Subtle Comb Strings",
    93: F  + r"\Pads\Bell Pad",
    94: F  + r"\Pads\Ghost Pad",
    95: F  + r"\Pads\Harmonic Sweep",
    96: F  + r"\Pads\Distant",
    97: F  + r"\Pads\Endgame",
    98: F  + r"\Pads\Sparkly",
    99: F  + r"\Pads\Distant",
    100: F + r"\Pads\Bright",
    101: F + r"\Pads\Yeti Funeral",
    102: F + r"\Pads\Ringing",
    103: F + r"\Pads\Computers In Space",
    105: F + r"\Leads\Banjo Remains",
    110: F + r"\Leads\Violini Solo",
    111: F + r"\Leads\Shanai",
    119: AZ + r"\Drums\Reverse Cymbal",
    127: AZ + r"\Drums\Kick Drum",
}


def resolve(program):
    """GM program number -> absolute patch path (no extension), always valid."""
    if program in OVERRIDES:
        return OVERRIDES[program]
    return FAMILY_DEFAULT[program // 8]


def validate():
    """Check every resolvable program points to a real .fxp; warn otherwise."""
    import os
    missing = set()
    for p in range(128):
        path = resolve(p) + ".fxp"
        if not os.path.isfile(path):
            missing.add(resolve(p))
    if missing:
        print(f"!! {len(missing)} mapped patch file(s) NOT found:")
        for m in sorted(missing):
            print("   ", m + ".fxp")
    else:
        print("All 128 GM programs map to existing patches. Good.")
    return not missing


def make_osc():
    return SimpleUDPClient(OSC_HOST, OSC_PORT)


def load_program(osc, program):
    path = resolve(program)
    osc.send_message("/patch/load", path)
    return path


def all_notes_off(osc):
    for n in range(128):
        osc.send_message("/mnote", [float(n), 0.0])


def run_bridge():
    osc = make_osc()
    names = mido.get_input_names()
    port = next((n for n in names if MIDI_PORT_MATCH in n), None)
    if not port:
        print("!! Macropad MIDI input not found. Ports:", names)
        sys.exit(1)
    # Open the Windows GM synth for real drum kits (channel 10).
    drum_out = None
    dname = next((n for n in mido.get_output_names() if DRUM_OUT_MATCH in n), None)
    if dname:
        try:
            drum_out = mido.open_output(dname)
            blog(f"Drum output: '{dname}' (channel 10 -> GM kit).")
        except Exception as e:
            blog(f"Could not open drum output: {e!r}")
    else:
        blog("No GM synth output found; drums will fall back to Surge.")

    print(f"Bridge running. Owning '{port}', forwarding to Surge via OSC {OSC_HOST}:{OSC_PORT}.")
    all_notes_off(osc)  # clean slate on startup
    # The bridge OWNS the Pico input (Windows MIDI is single-client), so it must
    # forward notes to Surge too -- surge-cli runs OSC-only, not bound to MIDI.
    with mido.open_input(port) as inp:
        for msg in inp:
            if msg.type in ("note_on", "note_off"):
                if msg.channel == DRUM_CHANNEL and drum_out is not None:
                    drum_out.send(msg)  # real GM drum kit
                elif msg.type == "note_on":
                    osc.send_message("/mnote", [float(msg.note), float(msg.velocity)])
                else:
                    osc.send_message("/mnote", [float(msg.note), 0.0])
            elif msg.type == "control_change":
                # Channel-10 CCs (e.g. volume) go to the GM drum synth.
                if msg.channel == DRUM_CHANNEL and drum_out is not None:
                    drum_out.send(msg)
                    continue
                v = msg.value / 127.0
                c = msg.control
                if   c == 7:  osc.send_message("/param/global/volume", v)        # Volume
                elif c == 74: osc.send_message("/param/a/filter/1/cutoff", v)     # Filter
                elif c == 71: osc.send_message("/param/a/filter/1/resonance", v)  # Resonance
                elif c == 91: osc.send_message("/param/fx/send/1/return", v)      # Reverb send
                elif c == 93: osc.send_message("/param/fx/send/2/return", v)      # Delay send
                elif c in (120, 123): all_notes_off(osc)
            elif msg.type == "pitchwheel":  # Pitch knob -> Surge pitch bend
                osc.send_message("/pbend", [0.0, msg.pitch / 8192.0])
            elif msg.type == "program_change":
                path = load_program(osc, msg.program)
                blog(f"PC {msg.program:3d} -> {path.split(chr(92))[-1]}")


def run_test():
    osc = make_osc()
    demo = [0, 24, 40, 48, 56, 65, 33, 80, 89]  # piano..warm pad
    NOTE_NAMES = "C C# D D# E F F# G G# A A# B".split()
    print("=== bridge OSC test: cycling GM instruments ===")
    for prog in demo:
        path = load_program(osc, prog)
        print(f"  GM {prog:3d} -> {path.split(chr(92))[-1]}")
        time.sleep(1.6)
        for n in (60, 64, 67, 72):
            osc.send_message("/mnote", [float(n), 100.0]); time.sleep(0.4)
            osc.send_message("/mnote", [float(n), 0.0]);   time.sleep(0.05)
        for n in (60, 64, 67):
            osc.send_message("/mnote", [float(n), 100.0])
        time.sleep(1.2)
        for n in (60, 64, 67):
            osc.send_message("/mnote", [float(n), 0.0])
        time.sleep(0.5)
    print("Done.")


if __name__ == "__main__":
    validate()
    if "--test" in sys.argv:
        run_test()
    else:
        run_bridge()
