// Captures the README demo video's frames from the UI demo mode, driven by
// timeline.json (same timeline as the soundtrack).
//   npm run dev   (UI on http://localhost:1420, demo mode in a plain browser)
//   node scripts/demo-video/capture.mjs <frames-dir> [edge.exe]
// Needs puppeteer-core (devDependency) and Microsoft Edge or Chrome.
import puppeteer from "puppeteer-core";
import { spawn } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

const OUT = path.resolve(process.argv[2] ?? "frames");
const BROWSER = process.argv[3] ?? "C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe";
const FPS = 30;
const tl = JSON.parse(fs.readFileSync(new URL("./timeline.json", import.meta.url)));
fs.rmSync(OUT, { recursive: true, force: true });
fs.mkdirSync(OUT, { recursive: true });

// Our own headless browser + attach (works even when the user's browser is open).
const PORT = 9337;
spawn(BROWSER, ["--headless=new", `--remote-debugging-port=${PORT}`, `--user-data-dir=${path.join(OUT, "..", "browser-profile")}`,
  "--no-first-run", "--hide-scrollbars", "about:blank"], { stdio: "ignore" });
let browser;
for (let i = 0; i < 50 && !browser; i++) {
  await new Promise((r) => setTimeout(r, 300));
  const v = await fetch(`http://127.0.0.1:${PORT}/json/version`).then((r) => r.json()).catch(() => null);
  if (v) browser = await puppeteer.connect({ browserWSEndpoint: v.webSocketDebuggerUrl });
}
const page = await browser.newPage();
await page.setViewport({ width: 1280, height: 800, deviceScaleFactor: 1.25 });
await page.goto("http://localhost:1420/?scene=main", { waitUntil: "networkidle0" });
await page.waitForSelector(".pad-grid .pad");
await new Promise((r) => setTimeout(r, 600));

const lane = (name, t) => {
  const pts = tl.auto[name];
  for (let i = 1; i < pts.length; i++) {
    if (t <= pts[i][0]) {
      const [t0, v0] = pts[i - 1], [t1, v1] = pts[i];
      return t1 > t0 ? v0 + (v1 - v0) * Math.min(1, Math.max(0, (t - t0) / (t1 - t0))) : v1;
    }
  }
  return pts[pts.length - 1][1];
};
const clickTab = (label) => page.evaluate((label) => {
  [...document.querySelectorAll(".tab")].find((t) => t.textContent === label)?.click();
}, label);

let nextUi = 0, lastLayers = -1, lastEcho = -1, lastFv = -1;
await page.evaluate(() => window.padiDemo.apply({ studio: { armed: false, recordingPad: null, layerMutes: [], metronome: false, bpm: 140, bars: 2 } }));

const frames = Math.ceil(tl.seconds * FPS);
for (let i = 0; i < frames; i++) {
  const t = i / FPS;
  const state = {};
  while (nextUi < tl.ui.length && tl.ui[nextUi].t <= t) {
    const k = tl.ui[nextUi++];
    Object.assign(state, { device: { ...(state.device ?? {}), ...k.device }, studio: { ...(state.studio ?? {}), ...k.studio } });
    if (k.sound) state.sound = k.sound;
    if (k.drumKit) state.drumKit = k.drumKit;
    if (k.tab) await clickTab(k.tab);
  }
  // Knob = filter, editor Echo slider.
  const fv = Math.round(lane("cutoff", t) * 127);
  if (fv !== lastFv) { state.device = { ...(state.device ?? {}), fx: 0, fv }; lastFv = fv; }
  const echo = Math.round(lane("echo", t) * 100) / 100;
  if (echo !== lastEcho) { state.params = { echo }; lastEcho = echo; }
  // Looper: layers, recording, position.
  const L = tl.loop;
  const layers = L.layers.filter(([at]) => at <= t).pop()?.[1] ?? 0;
  if (layers !== lastLayers) { state.studio = { ...(state.studio ?? {}), layerMutes: Array(layers).fill(false) }; lastLayers = layers; }
  const recording = L.rec.some(([a, b]) => t >= a && t < b);
  state.transport = { pos: (t % L.len) / L.len, seconds: L.len, playing: true, recording, countinBeats: 0, mic: 0 };
  // Pads and keys being played right now.
  state.pads = [...new Set(tl.pads.filter((p) => t >= p.t && t < p.t + p.d).map((p) => p.pad))];
  state.notes = tl.tracks.lead.filter((n) => t >= n.t && t < n.t + Math.min(n.d, 0.25)).map((n) => n.k);

  await page.evaluate((s) => window.padiDemo.apply(s), state);
  await new Promise((r) => setTimeout(r, 20));
  await page.screenshot({ path: path.join(OUT, `${String(i).padStart(4, "0")}.jpg`), type: "jpeg", quality: 92 });
}
await browser.close();
console.log(`${frames} frames -> ${OUT}`);
