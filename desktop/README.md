# PADI desktop app

Tauri 2 (Rust) + React. See the [main README](../README.md#build-the-app-from-source) for setup.

- `src-tauri/src/engine.rs`: audio engine (SoundFont synths, sampler, echo, looper, recorder tap)
- `src-tauri/src/midi.rs`: pad MIDI link and routing
- `src-tauri/src/device.rs`: pad serial link (live view, mode commands)
- `src-tauri/src/studio.rs`: kits, mic recording, My sounds, looper control, WAV export
- `src/`: React UI (`App.tsx`, `studio.tsx`, `editor.tsx`, `menu.tsx`)
- `scripts/get-packs.sh`: downloads the FreePats sound packs into `src-tauri/resources/packs`
