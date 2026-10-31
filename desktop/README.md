# PADI desktop app

Tauri 2 (Rust) + React. See the [main README](../README.md#build-the-app-from-source) for setup.

- `src-tauri/src/engine.rs`: audio engine (SoundFont synths, sampler, echo, looper, recorder tap)
- `src-tauri/src/midi.rs`: pad MIDI link and routing
- `src-tauri/src/device.rs`: pad serial link (live view, mode commands)
- `src-tauri/src/studio.rs`: kits, mic recording, My sounds, looper control, WAV export
- `src/`: React UI (`App.tsx`, `studio.tsx`, `editor.tsx`, `menu.tsx`)
- `scripts/get-packs.sh`: downloads the FreePats sound packs into `src-tauri/resources/packs`

## Demo video (README)

`scripts/demo-video/` makes the README video from one timeline, so picture and sound stay in sync:

```bash
node scripts/demo-video/timeline.mjs                       # writes timeline.json (the song + UI keyframes)
npm run dev                                                # UI demo mode on :1420
node scripts/demo-video/capture.mjs ../out/frames          # 30 fps frames via headless Edge/Chrome
cd src-tauri && cargo run --release --example demo_audio -- ../scripts/demo-video/timeline.json ../../out/audio.wav
ffmpeg -framerate 30 -i out/frames/%04d.jpg -i out/audio.wav -c:v libx264 -pix_fmt yuv420p -c:a aac -shortest padi-demo.mp4
```
