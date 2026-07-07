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
TP = BASE + r"\patches_3rdparty"                      # 3rd-party packs (the gems)
AZ = TP + r"\Aleksey Zhehanov"                        # realistic acoustic GM set
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


# ---------------------------------------------------------------------------
#  Surge instrument catalog: 6 categories x 8 curated patches.
#  Order MUST match catalog.h (CAT_NAMES / CAT_PATCH_NAMES) on the device.
#  The device sends Bank Select (CC0 = category index) + Program Change (patch).
# ---------------------------------------------------------------------------
CATALOG = [
    [  # 0 Keys
        AZ + r"\Keys\Grand Piano", AZ + r"\Keys\Upright Piano", AZ + r"\Keys\Soft Piano",
        F + r"\Keys\EP 1", F + r"\Keys\DX EP", F + r"\Keys\Soft Suitcase",
        F + r"\Keys\Organ 1", F + r"\Keys\Church",
    ],
    [  # 1 Pads
        F + r"\Pads\MKS-70 Warm Pad", F + r"\Pads\Pad 1", F + r"\Pads\Bright",
        F + r"\Pads\Distant", F + r"\Pads\Ghost Pad", F + r"\Pads\Harmonic Sweep",
        F + r"\Pads\Bell Pad", F + r"\Pads\Ooh",
    ],
    [  # 2 Leads
        F + r"\Leads\Classic Lead 1", F + r"\Leads\Classic Lead 2", F + r"\Leads\Crisp PWM",
        F + r"\Leads\Digi It", F + r"\Leads\Butter", F + r"\Leads\Fairy",
        F + r"\Leads\FM Rock", F + r"\Leads\Dome",
    ],
    [  # 3 Bass
        F + r"\Basses\Bass 1", F + r"\Basses\E-Bass", F + r"\Basses\Rubber Bass",
        F + r"\Basses\FM Bass 1", F + r"\Basses\Deep End", F + r"\Basses\Distorted Bass",
        F + r"\Basses\Digibass", F + r"\Basses\Piano Bass",
    ],
    [  # 4 Plucks
        F + r"\Plucks\Bell 1", F + r"\Plucks\FM Pluck", F + r"\Plucks\E-Guitar",
        F + r"\Plucks\Clean", F + r"\Plucks\Diamonds", F + r"\Plucks\Delay Dancer",
        F + r"\Plucks\Fantasy Bell", F + r"\Plucks\80s Gliss",
    ],
    [  # 5 Poly
        F + r"\Polysynths\Jupiter-8", F + r"\Polysynths\Juno-60 Strings", F + r"\Polysynths\Korg Triton",
        F + r"\Polysynths\Fast Poly", F + r"\Polysynths\FM Poly", F + r"\Polysynths\Fat Curtis",
        F + r"\Polysynths\Ahh Polly", F + r"\Polysynths\Gentle",
    ],
    [  # 6 Arps (self-playing sequences)
        F + r"\Sequences\Acid Seq 1", F + r"\Sequences\Octave Arp", F + r"\Sequences\Bell Seq",
        F + r"\Sequences\Multi Seq", F + r"\Sequences\FM Seq", F + r"\Sequences\Game On 1",
        F + r"\Sequences\Gate Chord", F + r"\Sequences\Fifth Seq",
    ],
    [  # 7 Space (atmospheres / cinematic)
        TP + r"\Rare Earth\FX\Deep Note", F + r"\FX\Space Adventure 1", F + r"\FX\Space Cadet",
        F + r"\FX\Aliens", F + r"\FX\XG Stardust", TP + r"\Luna\Atmospheres\Alias Glass Voice 1",
        TP + r"\Luna\Atmospheres\Pretty Fifth", F + r"\FX\Vinyl",
    ],
    [  # 8 SynBass (boutique 3rd-party basses)
        TP + r"\Luna\Basses\Chonky Analog Bass 1", TP + r"\Luna\Basses\Slap Bass", TP + r"\Luna\Basses\Synthwave",
        TP + r"\Nick Moritz\Basses\Juno Fire", TP + r"\Nick Moritz\Basses\Hoover FM", TP + r"\Vincent Zauhar\Basses\Electro",
        TP + r"\Databroth\Basses\Crystal Rip", TP + r"\Luna\Basses\Screamy Bass",
    ],
    [  # 9 Signat (showpiece keys/pads/leads)
        TP + r"\Vincent Zauhar\Pads\Classic Warm Jupiters", TP + r"\Vincent Zauhar\Keys\Blue Lines, Red Lights 1",
        TP + r"\Vincent Zauhar\Keys\So Appalled", TP + r"\Vincent Zauhar\Keys\Viva La Trance",
        TP + r"\Vincent Zauhar\Pads\80s Brass", TP + r"\Argitoth\Leads\Trance Lead 1",
        TP + r"\Argitoth\Pads\Blue Pad", TP + r"\Rare Earth\Plucks\Music Box",
    ],
]


def resolve(program):
    """GM program number -> absolute patch path (no extension), always valid."""
    if program in OVERRIDES:
        return OVERRIDES[program]
    return FAMILY_DEFAULT[program // 8]


def resolve_catalog(bank, program):
    """(category, patch) -> patch path, or None if out of range."""
    if 0 <= bank < len(CATALOG) and 0 <= program < len(CATALOG[bank]):
        return CATALOG[bank][program]
    return None


def validate():
    """Check every resolvable program points to a real .fxp; warn otherwise."""
    import os
    missing = set()
    for p in range(128):
        path = resolve(p) + ".fxp"
        if not os.path.isfile(path):
            missing.add(resolve(p))
    for cat in CATALOG:
        for path in cat:
            if not os.path.isfile(path + ".fxp"):
                missing.add(path)
    if missing:
        print(f"!! {len(missing)} mapped patch file(s) NOT found:")
        for m in sorted(missing):
            print("   ", m + ".fxp")
    else:
        print("All GM programs + catalog patches map to existing files. Good.")
    return not missing


# ---------------------------------------------------------------------------
#  Knob-FX routing. The macropad's knob sends standard MIDI CCs (so it works
#  on any host). Here we translate each into a Surge effect on scene-A insert
#  slot 1 (/param/fx/a/1/...), retyping the slot as the effect changes.
# ---------------------------------------------------------------------------
FX_NATIVE = {                       # CC -> Surge scene param (0..1)
    74: "/param/a/filter/1/cutoff",
    71: "/param/a/filter/1/resonance",
}
FX_INSERT = {                       # CC -> (fx type index, mix/drive param #)
    91: (11, 10),   # Reverb 2  -> param10 Mix
    93: (9, 7),     # Chorus    -> param7  Mix
    85: (1, 11),    # Delay     -> param11 Mix
    86: (3, 7),     # Phaser    -> param7  Mix
    87: (5, 5),     # Distortion-> param5  Drive
}


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
    # The knob no longer controls volume, so keep Surge's master at full and let
    # the user set level on the PC. (Also re-applied after every patch load.)
    osc.send_message("/param/global/volume", 1.0)
    pending_bank = None  # last Bank Select (CC0) = catalog category
    loaded_fx = None     # FX type currently in insert slot a/1 (0 = off)

    def set_insert(fxtype):
        nonlocal loaded_fx
        if loaded_fx != fxtype:
            osc.send_message("/param/fx/a/1/type", float(fxtype))
            loaded_fx = fxtype
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
                if msg.control == 0:            # Bank Select MSB = catalog category
                    pending_bank = msg.value
                    continue
                v = msg.value / 127.0
                c = msg.control
                if c == 7:
                    osc.send_message("/param/global/volume", v)          # Volume (legacy)
                elif c in FX_NATIVE:                                     # Filter / Resonance
                    set_insert(0)                                       # drop any insert FX
                    osc.send_message(FX_NATIVE[c], v)
                elif c in FX_INSERT:                                     # Reverb/Chorus/Delay/...
                    fxtype, mixp = FX_INSERT[c]
                    set_insert(fxtype)
                    osc.send_message(f"/param/fx/a/1/param{mixp}", v)
                elif c in (120, 123): all_notes_off(osc)
            elif msg.type == "pitchwheel":  # Pitch knob -> Surge pitch bend
                osc.send_message("/pbend", [0.0, msg.pitch / 8192.0])
            elif msg.type == "program_change":
                path = resolve_catalog(pending_bank, msg.program) if pending_bank is not None else None
                if path is None:                    # no/invalid bank -> GM fallback
                    path = resolve(msg.program)
                    tag = f"PC {msg.program:3d}"
                else:
                    tag = f"cat {pending_bank}/{msg.program}"
                osc.send_message("/patch/load", path)
                osc.send_message("/param/global/volume", 1.0)  # keep master up (knob != volume now)
                loaded_fx = None    # patch load resets Surge FX; re-type on next FX CC
                blog(f"{tag} -> {path.split(chr(92))[-1]}")


def run_test():
    osc = make_osc()
    # One patch from each catalog category (cat, patch).
    demo = [(0, 0), (0, 3), (1, 0), (2, 0), (3, 0), (4, 1), (5, 0), (5, 1)]
    print("=== bridge OSC test: cycling catalog instruments ===")
    for bank, prog in demo:
        path = resolve_catalog(bank, prog)
        osc.send_message("/patch/load", path)
        print(f"  cat {bank}/{prog} -> {path.split(chr(92))[-1]}")
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
