"""
Macropad power-mode supervisor (Windows).

Keeps the headless Surge engine + the MIDI->Surge bridge running, and makes the
whole thing plug-and-play:
  * launches surge-xt-cli (OSC-only sound engine) and keeps it alive
  * watches for the Pico ("Macro MIDI Pad") appearing/disappearing
  * starts the bridge when the Pico is connected, and restarts it on every
    reconnect (Windows MIDI is single-client, so the bridge must reopen the
    fresh handle each time)

Run headless at login via start_macropad.vbs. Logs to supervisor.log next to it.
"""
import os, sys, time, subprocess, datetime
import mido

HERE = os.path.dirname(os.path.abspath(__file__))
VENV_PY = os.path.join(HERE, ".venv", "Scripts", "python.exe")
BRIDGE = os.path.join(HERE, "midi_surge_bridge.py")
LOGFILE = os.path.join(HERE, "supervisor.log")
SURGE_LOG = os.path.join(HERE, "surge.log")

SURGE_DIR = r"C:\Users\rio18\Tools\SurgeXT\Surge Synth Team"
SURGE_EXE = os.path.join(SURGE_DIR, "surge-xt-cli.exe")
# WASAPI shared (1.0): low latency AND mixes with other apps. (DirectSound had
# ~5s latency; earlier "YouTube silent" was likely the Windows default output
# being the HyperX headphones, not Surge hogging the device.)
SURGE_ARGS = ["--audio-interface=1.0", "--osc-in-port=53280", "--no-stdin"]
MATCH = "Macro MIDI Pad"

CREATE_NO_WINDOW = 0x08000000  # keep child consoles hidden


def log(msg):
    line = f"{datetime.datetime.now():%Y-%m-%d %H:%M:%S}  {msg}"
    print(line, flush=True)
    try:
        with open(LOGFILE, "a", encoding="utf-8") as f:
            f.write(line + "\n")
    except OSError:
        pass


def pico_present():
    try:
        return any(MATCH in n for n in mido.get_input_names())
    except Exception:
        return False


_surge_log_fh = None

def start_surge():
    """Launch surge-cli, logging to surge.log so we can confirm audio opened."""
    global _surge_log_fh
    if _surge_log_fh:
        try: _surge_log_fh.close()
        except OSError: pass
    _surge_log_fh = open(SURGE_LOG, "w", encoding="utf-8")
    return subprocess.Popen([SURGE_EXE] + SURGE_ARGS, cwd=SURGE_DIR,
                            stdout=_surge_log_fh, stderr=subprocess.STDOUT,
                            creationflags=CREATE_NO_WINDOW)


def surge_audio_ok():
    try:
        with open(SURGE_LOG, encoding="utf-8", errors="ignore") as fh:
            return "Audio Starting" in fh.read()
    except OSError:
        return False


def start_surge_verified():
    """Start surge and retry until its audio device actually opens (it can fail
    silently if the device wasn't released yet from a prior instance)."""
    for attempt in range(6):
        p = start_surge()
        for _ in range(8):          # wait up to ~8s for "Audio Starting"
            time.sleep(1)
            if p.poll() is not None:
                break               # exited (e.g. audio open error) -> retry
            if surge_audio_ok():
                log(f"surge-xt-cli audio OK (attempt {attempt + 1}).")
                return p
        log(f"surge audio not confirmed (attempt {attempt + 1}); retrying.")
        try: p.terminate()
        except Exception: pass
        time.sleep(3)               # let the audio device release
    log("!! surge audio failed after retries; will keep trying.")
    return p


def start_bridge():
    return subprocess.Popen([VENV_PY, "-u", BRIDGE],
                            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                            creationflags=CREATE_NO_WINDOW)


def alive(p):
    return p is not None and p.poll() is None


def main():
    log("Supervisor started.")
    surge = None
    bridge = None
    was_present = False
    while True:
        # 1) keep the sound engine alive (with verified audio)
        if not alive(surge):
            if surge is not None:
                time.sleep(3)  # let the audio device release before relaunch
            surge = start_surge_verified()

        # 2) manage the bridge around Pico presence
        present = pico_present()
        if present and not alive(bridge):
            bridge = start_bridge()
            log("Pico connected -> bridge started.")
        elif not present and alive(bridge):
            bridge.terminate()
            bridge = None
            log("Pico disconnected -> bridge stopped.")
        elif was_present and not present:
            log("Pico disconnected.")
        was_present = present

        time.sleep(2)


if __name__ == "__main__":
    main()
