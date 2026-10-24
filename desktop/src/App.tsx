import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { disable as disableAutostart, enable as enableAutostart, isEnabled as autostartEnabled } from "@tauri-apps/plugin-autostart";
import {
  BEAT_NAMES, DEFAULT_STATE, DeviceState, FX_NAMES, GM_FAMILIES, MENU_PAGES, modeName, noteName, padChannel, padLabel, padNotes, Style,
} from "./music";
import "./App.css";

interface Sound { bank: number; program: number; name: string }
interface Status {
  engine: { ok: boolean; output: string; sampleRate: number; error: string | null };
  picoMidi: boolean;
  picoSerial: boolean;
  serialPaused: boolean;
  sound: Sound | null;
  drumKit: Sound | null;
  volume: number;
  device: DeviceState;
}
interface MidiEvent { kind: "on" | "off" | "cc" | "bend" | "pc"; ch: number; a: number; b: number }
interface PadEvent { pad: number; down: boolean }

// Pad 12-15 is the top row on the device; draw top-down.
const PAD_ROWS = [[12, 13, 14, 15], [8, 9, 10, 11], [4, 5, 6, 7], [0, 1, 2, 3]];

export default function App() {
  const [status, setStatus] = useState<Status | null>(null);
  const [presets, setPresets] = useState<Sound[]>([]);
  const [pads, setPads] = useState<Set<number>>(new Set());
  const [notes, setNotes] = useState<Set<number>>(new Set());
  const [log, setLog] = useState<string[]>([]);

  useEffect(() => {
    invoke<Status>("get_status").then(setStatus);
    invoke<Sound[]>("get_presets").then(setPresets);
    invoke<string[]>("get_log").then(setLog);
    const unlisten = [
      listen<Status>("status", (e) => setStatus(e.payload)),
      listen<string>("log", (e) => setLog((l) => [...l.slice(-199), e.payload])),
      listen<PadEvent>("pad", (e) =>
        setPads((s) => toggle(s, e.payload.pad, e.payload.down))),
      listen<MidiEvent>("midi", (e) => {
        const m = e.payload;
        if (m.kind === "on" || m.kind === "off") setNotes((s) => toggle(s, m.a, m.kind === "on"));
        if (m.kind === "cc" && (m.a === 120 || m.a === 123)) setNotes(new Set());
      }),
    ];
    return () => unlisten.forEach((p) => p.then((f) => f()));
  }, []);

  // Device link dropped: nothing is held any more.
  useEffect(() => {
    if (!status?.picoSerial) setPads(new Set());
  }, [status?.picoSerial]);

  const dev = status && Object.keys(status.device).length ? status.device : DEFAULT_STATE;
  const live = !!status?.picoSerial;

  return (
    <div className="app">
      <Header status={status} />
      <main className="main">
        <SoundBrowser presets={presets} status={status} />
        <section className="stage">
          <NowPlaying status={status} dev={dev} live={live} />
          <div className="stage-row">
            <PadGrid dev={dev} pads={pads} setPads={setPads} setNotes={setNotes} live={live} />
            <aside className="side">
              <Knob dev={dev} live={live} />
              <Activity log={log} />
            </aside>
          </div>
          <Keyboard notes={notes} />
        </section>
      </main>
    </div>
  );
}

function toggle(s: Set<number>, v: number, on: boolean) {
  const n = new Set(s);
  if (on) n.add(v); else n.delete(v);
  return n;
}

function Header({ status }: { status: Status | null }) {
  const [autostart, setAutostart] = useState(false);
  useEffect(() => { autostartEnabled().then(setAutostart).catch(() => {}); }, []);
  const flipAutostart = async () => {
    if (autostart) await disableAutostart(); else await enableAutostart();
    setAutostart(await autostartEnabled());
  };
  const engine = status?.engine;
  return (
    <header className="header">
      <div className="brand">
        <span className="brand-mark" aria-hidden>
          {Array.from({ length: 4 }, (_, i) => <i key={i} />)}
        </span>
        PADI
      </div>
      <div className="chips">
        <Chip on={!!status?.picoMidi} label="Pad" detail={status?.picoMidi ? "connected" : "plug in the pad"} />
        <Chip
          on={!!status?.picoSerial}
          warn={status?.serialPaused}
          label="Live view"
          detail={status?.serialPaused ? "port released" : status?.picoSerial ? "on" : "waiting"}
        />
        <Chip on={!!engine?.ok} label="Sound" detail={engine?.ok ? engine.output : engine?.error ?? "starting"} />
      </div>
      <div className="header-tools">
        <label className="volume" title="Master volume">
          <span>Vol</span>
          <input
            type="range" min={0} max={1.2} step={0.01}
            value={status?.volume ?? 0.8}
            onChange={(e) => invoke("set_volume", { volume: Number(e.target.value) })}
          />
        </label>
        <button className="btn ghost" onClick={() => invoke("panic")}>Stop sound</button>
        <details className="menu">
          <summary className="btn ghost">⋯</summary>
          <div className="menu-pop">
            <label className="check">
              <input type="checkbox" checked={autostart} onChange={flipAutostart} />
              Start with Windows (in the tray)
            </label>
            <label className="check">
              <input
                type="checkbox"
                checked={!!status?.serialPaused}
                onChange={(e) => invoke("set_serial_paused", { paused: e.target.checked })}
              />
              Release pad port (to flash firmware)
            </label>
          </div>
        </details>
      </div>
    </header>
  );
}

function Chip({ on, warn, label, detail }: { on: boolean; warn?: boolean; label: string; detail: string }) {
  return (
    <div className={`chip ${warn ? "warn" : on ? "on" : "off"}`} title={detail}>
      <span className="dot" />
      <b>{label}</b>
      <span className="chip-detail">{detail}</span>
    </div>
  );
}

function SoundBrowser({ presets, status }: { presets: Sound[]; status: Status | null }) {
  const [query, setQuery] = useState("");
  const [family, setFamily] = useState<number | "drums" | "all">("all");

  const groups = useMemo(() => {
    const q = query.trim().toLowerCase();
    return presets.filter((p) => {
      if (q && !p.name.toLowerCase().includes(q)) return false;
      if (family === "all") return true;
      if (family === "drums") return p.bank === 128;
      return p.bank !== 128 && Math.floor(p.program / 8) === family;
    });
  }, [presets, query, family]);

  const isCurrent = (p: Sound) => {
    const cur = p.bank === 128 ? status?.drumKit : status?.sound;
    return !!cur && cur.bank === p.bank && cur.program === p.program;
  };

  return (
    <nav className="browser">
      <div className="browser-head">
        <h2>Sounds</h2>
        <span className="muted">{presets.length}</span>
      </div>
      <input className="search" placeholder="Search sounds…" value={query} onChange={(e) => setQuery(e.target.value)} />
      <div className="families">
        <button className={family === "all" ? "fam active" : "fam"} onClick={() => setFamily("all")}>All</button>
        {GM_FAMILIES.map((f, i) => (
          <button key={f} className={family === i ? "fam active" : "fam"} onClick={() => setFamily(i)}>{f}</button>
        ))}
        <button className={family === "drums" ? "fam active" : "fam"} onClick={() => setFamily("drums")}>Drum kits</button>
      </div>
      <ul className="presets">
        {groups.map((p) => (
          <li key={`${p.bank}-${p.program}`}>
            <button className={isCurrent(p) ? "preset current" : "preset"} onClick={() => invoke("set_sound", { bank: p.bank, program: p.program })}>
              <span className="preset-name">{p.name}</span>
              <span className="preset-tag">
                {p.bank === 128 ? "kit" : p.bank === 0 ? GM_FAMILIES[Math.floor(p.program / 8)] : `var ${p.bank}`}
              </span>
            </button>
          </li>
        ))}
        {!groups.length && <li className="muted empty">No sounds match.</li>}
      </ul>
    </nav>
  );
}

function NowPlaying({ status, dev, live }: { status: Status | null; dev: DeviceState; live: boolean }) {
  const oct = dev.oc;
  // In Drums mode the pads play the kit, so that's the sound that matters.
  const drums = live && dev.st === Style.Drums;
  const main = drums ? status?.drumKit : status?.sound;
  const other = drums ? status?.sound : status?.drumKit;
  return (
    <div className="now">
      <div className="now-main">
        <span className="eyebrow">Now playing</span>
        <h1>{main?.name ?? "—"}</h1>
        <span className="muted">{drums ? "Instrument" : "Drum kit"}: {other?.name ?? "—"}</span>
      </div>
      <dl className="now-facts">
        <div><dt>Mode</dt><dd>{live ? modeName(dev) : "—"}</dd></div>
        <div><dt>Octave</dt><dd>{live ? (oct > 0 ? `+${oct}` : oct) : "—"}</dd></div>
        <div><dt>Transpose</dt><dd>{live ? (dev.tr > 0 ? `+${dev.tr}` : dev.tr) : "—"}</dd></div>
        <div><dt>Beat</dt><dd>{live ? BEAT_NAMES[dev.bt] ?? "—" : "—"}</dd></div>
      </dl>
    </div>
  );
}

function PadGrid({ dev, pads, setPads, setNotes, live }: {
  dev: DeviceState;
  pads: Set<number>;
  setPads: React.Dispatch<React.SetStateAction<Set<number>>>;
  setNotes: React.Dispatch<React.SetStateAction<Set<number>>>;
  live: boolean;
}) {
  // Notes started by an on-screen click, so release stops exactly those.
  const clickNotes = useRef(new Map<number, { ch: number; notes: number[] }>());
  const inMenu = !!dev.mn;

  const press = (pad: number) => {
    if (inMenu) return;
    const ch = padChannel(dev);
    const ns = padNotes(dev, pad);
    clickNotes.current.set(pad, { ch, notes: ns });
    ns.forEach((note) => invoke("play_note", { ch, note, velocity: dev.ve || 100, on: true }));
    setPads((s) => toggle(s, pad, true));
    setNotes((s) => { const n = new Set(s); ns.forEach((x) => n.add(x)); return n; });
  };
  const release = (pad: number) => {
    const held = clickNotes.current.get(pad);
    if (!held) return;
    clickNotes.current.delete(pad);
    held.notes.forEach((note) => invoke("play_note", { ch: held.ch, note, velocity: 0, on: false }));
    setPads((s) => toggle(s, pad, false));
    setNotes((s) => { const n = new Set(s); held.notes.forEach((x) => n.delete(x)); return n; });
  };

  return (
    <div className={`pad-card ${inMenu ? "menu-mode" : ""}`}>
      <div className="pad-card-head">
        <h3>{inMenu ? `Pad menu · ${MENU_PAGES[dev.pg] ?? ""}` : "Pads"}</h3>
        <span className="muted">{live ? (inMenu ? "browsing on the pad" : "live") : "click to play"}</span>
      </div>
      <div className="pad-grid">
        {PAD_ROWS.flat().map((pad) => {
          const label = padLabel(dev, pad);
          return (
            <button
              key={pad}
              className={`pad ${pads.has(pad) ? "down" : ""} ${label.role ?? ""}`}
              onPointerDown={(e) => { e.currentTarget.setPointerCapture(e.pointerId); press(pad); }}
              onPointerUp={() => release(pad)}
              onPointerCancel={() => release(pad)}
            >
              <span className="pad-num">{pad + 1}</span>
              <span className="pad-main">{label.main}</span>
              {label.sub && <span className="pad-sub">{label.sub}</span>}
            </button>
          );
        })}
      </div>
    </div>
  );
}

function Knob({ dev, live }: { dev: DeviceState; live: boolean }) {
  const pct = live ? Math.round((dev.fv / 127) * 100) : 0;
  return (
    <div className="card knob">
      <div className="card-head">
        <h3>Knob</h3>
        <span className="muted">{live ? FX_NAMES[dev.fx] : "—"}</span>
      </div>
      <div className="meter"><div className="meter-fill" style={{ width: `${pct}%` }} /></div>
      <div className="knob-value">{live ? dev.fv : "—"}<span className="muted"> / 127</span></div>
    </div>
  );
}

function Activity({ log }: { log: string[] }) {
  const ref = useRef<HTMLOListElement>(null);
  useEffect(() => { ref.current?.scrollTo({ top: ref.current.scrollHeight }); }, [log]);
  return (
    <div className="card activity">
      <div className="card-head"><h3>Activity</h3></div>
      <ol ref={ref}>
        {log.map((l, i) => <li key={i}>{l}</li>)}
      </ol>
    </div>
  );
}

const KB_LOW = 36;  // C2
const KB_HIGH = 96; // C7
const IS_BLACK = [false, true, false, true, false, false, true, false, true, false, true, false];

function Keyboard({ notes }: { notes: Set<number> }) {
  const whites: number[] = [];
  for (let n = KB_LOW; n <= KB_HIGH; n++) if (!IS_BLACK[n % 12]) whites.push(n);
  const w = 100 / whites.length;
  return (
    <div className="keyboard" aria-label="Notes playing">
      {whites.map((n) => (
        <div key={n} className={`key white ${notes.has(n) ? "on" : ""}`} style={{ width: `${w}%` }}>
          {n % 12 === 0 && <span>{noteName(n)}</span>}
        </div>
      ))}
      {Array.from({ length: KB_HIGH - KB_LOW + 1 }, (_, i) => KB_LOW + i)
        .filter((n) => IS_BLACK[n % 12])
        .map((n) => {
          const leftWhites = whites.filter((x) => x < n).length;
          return (
            <div key={n} className={`key black ${notes.has(n) ? "on" : ""}`} style={{ left: `${leftWhites * w - w * 0.3}%`, width: `${w * 0.6}%` }} />
          );
        })}
    </div>
  );
}
