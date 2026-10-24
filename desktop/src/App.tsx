import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { disable as disableAutostart, enable as enableAutostart, isEnabled as autostartEnabled } from "@tauri-apps/plugin-autostart";
import {
  BEAT_NAMES, DEFAULT_STATE, DeviceState, FX_NAMES, GM_FAMILIES, MENU_PAGES, modeName, noteName, padChannel, padLabel, padNotes, Style,
} from "./music";
import { call, KitsPanel, LooperBar, PadEditor, PadView, StudioView, Transport, UndoRedo, updatePad, useStudio, Wave } from "./studio";
import { Sound, SoundEditor } from "./editor";
import { MenuProvider, useMenu } from "./menu";
import "./App.css";

interface Preset { font: number; bank: number; program: number; name: string }
interface Library { fonts: string[]; presets: Preset[] }
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
type SetPads = React.Dispatch<React.SetStateAction<Set<number>>>;

// Pad 12-15 is the top row on the device; draw top-down.
const PAD_ROWS = [[12, 13, 14, 15], [8, 9, 10, 11], [4, 5, 6, 7], [0, 1, 2, 3]];

export default function App() {
  return (
    <MenuProvider>
      <Padi />
    </MenuProvider>
  );
}

function Padi() {
  const [status, setStatus] = useState<Status | null>(null);
  const [library, setLibrary] = useState<Library>({ fonts: [], presets: [] });
  const [pads, setPads] = useState<Set<number>>(new Set());
  const [notes, setNotes] = useState<Set<number>>(new Set());
  const [lastLog, setLastLog] = useState("");
  const { studio, transport } = useStudio();
  const [tab, setTab] = useState<"sounds" | "kits">("sounds");
  const [selected, setSelected] = useState(0);

  useEffect(() => {
    const loadLibrary = () => invoke<Library>("get_presets").then(setLibrary);
    invoke<Status>("get_status").then(setStatus);
    invoke<string[]>("get_log").then((l) => setLastLog(l[l.length - 1] ?? ""));
    loadLibrary();
    const unlisten = [
      listen<Status>("status", (e) => setStatus(e.payload)),
      listen<string>("log", (e) => setLastLog(e.payload)),
      listen("library", loadLibrary),
      listen<PadEvent>("pad", (e) => setPads((s) => toggle(s, e.payload.pad, e.payload.down))),
      listen<MidiEvent>("midi", (e) => {
        const m = e.payload;
        if (m.kind === "on" || m.kind === "off") setNotes((s) => toggle(s, m.a, m.kind === "on"));
        if (m.kind === "cc" && (m.a === 120 || m.a === 123)) setNotes(new Set());
      }),
    ];
    // Ctrl+Z / Ctrl+Y (or Ctrl+Shift+Z) undo kit edits, outside text fields.
    const keys = (e: KeyboardEvent) => {
      if (!(e.ctrlKey || e.metaKey) || (e.target as HTMLElement).closest("input, textarea, select")) return;
      const k = e.key.toLowerCase();
      if (k === "z" || k === "y") {
        e.preventDefault();
        invoke("studio_undo", { redo: k === "y" || e.shiftKey });
      }
    };
    window.addEventListener("keydown", keys);
    return () => {
      unlisten.forEach((p) => p.then((f) => f()));
      window.removeEventListener("keydown", keys);
    };
  }, []);

  // Device link dropped: nothing is held any more.
  useEffect(() => {
    if (!status?.picoSerial) setPads(new Set());
  }, [status?.picoSerial]);

  const dev = status && Object.keys(status.device).length ? status.device : DEFAULT_STATE;
  const live = !!status?.picoSerial;
  // Pads play the kit when the pad is in Sampler mode (or, unplugged, when the
  // kit was last picked in the app). Recording always shows the kit.
  const sampler = (live ? dev.st === Style.Sampler : !!studio?.active) || !!studio?.armed;
  const drums = live && dev.st === Style.Drums;
  const knob = live ? `${FX_NAMES[dev.fx]} ${dev.fv}` : "—";

  return (
    <div className="app">
      <Header status={status} />
      <main className="main">
        <nav className="browser">
          <div className="tabs" role="tablist">
            <button role="tab" className={tab === "sounds" ? "tab on" : "tab"} onClick={() => setTab("sounds")}>Sounds</button>
            <button role="tab" className={tab === "kits" ? "tab on" : "tab"} onClick={() => setTab("kits")}>My kits</button>
          </div>
          {tab === "sounds" ? (
            <SoundBrowser library={library} status={status} />
          ) : (
            <KitsPanel studio={studio} currentSound={(drums ? status?.drumKit : status?.sound)?.mySound ?? null} />
          )}
        </nav>
        <section className="stage">
          <NowPlaying status={status} dev={dev} live={live} sampler={sampler} drums={drums} studio={studio} />
          <div className="stage-row">
            {sampler ? (
              <SamplerGrid studio={studio} transport={transport} pads={pads} setPads={setPads} selected={selected} setSelected={setSelected} />
            ) : (
              <PadGrid dev={dev} pads={pads} setPads={setPads} setNotes={setNotes} studio={studio} transport={transport} />
            )}
            <aside className="side">
              {sampler ? (
                <PadEditor pad={selected} sample={studio?.pads[selected] ?? null} studio={studio} />
              ) : (
                <SoundEditor sound={(drums ? status?.drumKit : status?.sound) ?? null} target={drums ? "drumKit" : "sound"} knob={knob} />
              )}
            </aside>
          </div>
          <LooperBar studio={studio} transport={transport} status={lastLog} />
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
        <span className="brand-mark" aria-hidden>{Array.from({ length: 4 }, (_, i) => <i key={i} />)}</span>
        PADI
      </div>
      <div className="chips">
        <Chip on={!!status?.picoMidi} label="Pad" detail={status?.picoMidi ? "connected" : "plug in the pad"} />
        <Chip on={!!status?.picoSerial} warn={status?.serialPaused} label="Live view"
          detail={status?.serialPaused ? "port released" : status?.picoSerial ? "on" : "waiting"} />
        <Chip on={!!engine?.ok} label="Sound" detail={engine?.ok ? engine.output : engine?.error ?? "starting"} />
      </div>
      <div className="header-tools">
        <label className="volume" title="Master volume">
          <span>Vol</span>
          <input type="range" min={0} max={1.2} step={0.01} value={status?.volume ?? 0.8}
            onChange={(e) => invoke("set_volume", { volume: Number(e.target.value) })} />
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
              <input type="checkbox" checked={!!status?.serialPaused}
                onChange={(e) => invoke("set_serial_paused", { paused: e.target.checked })} />
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

function SoundBrowser({ library, status }: { library: Library; status: Status | null }) {
  const openMenu = useMenu();
  const [query, setQuery] = useState("");
  const [family, setFamily] = useState<number | "drums" | "all">("all");
  const [font, setFont] = useState<number | "all">("all");
  const { presets, fonts } = library;

  const shown = useMemo(() => {
    const q = query.trim().toLowerCase();
    return presets.filter((p) => {
      if (q && !p.name.toLowerCase().includes(q)) return false;
      if (font !== "all" && p.font !== font) return false;
      if (family === "all") return true;
      if (family === "drums") return p.bank === 128;
      return p.bank !== 128 && Math.floor(p.program / 8) === family && p.font === 0;
    });
  }, [presets, query, family, font]);

  const isCurrent = (p: Preset) => {
    const cur = p.bank === 128 ? status?.drumKit : status?.sound;
    return !!cur && !cur.mySound && cur.font === p.font && cur.bank === p.bank && cur.program === p.program;
  };
  const use = (p: Preset) => invoke("set_sound", { font: p.font, bank: p.bank, program: p.program });
  const tag = (p: Preset) =>
    p.font > 0 ? fonts[p.font] : p.bank === 128 ? "kit" : p.bank === 0 ? GM_FAMILIES[Math.floor(p.program / 8)] : `var ${p.bank}`;

  return (
    <>
      <input className="search" placeholder="Search sounds…" value={query} onChange={(e) => setQuery(e.target.value)} />
      {fonts.length > 1 && (
        <div className="families">
          <button className={font === "all" ? "fam active" : "fam"} onClick={() => setFont("all")}>All packs</button>
          {fonts.map((f, i) => (
            <button key={f} className={font === i ? "fam active" : "fam"} onClick={() => setFont(i)}>{f}</button>
          ))}
        </div>
      )}
      <div className="families">
        <button className={family === "all" ? "fam active" : "fam"} onClick={() => setFamily("all")}>All</button>
        {GM_FAMILIES.map((f, i) => (
          <button key={f} className={family === i ? "fam active" : "fam"} onClick={() => setFamily(i)}>{f}</button>
        ))}
        <button className={family === "drums" ? "fam active" : "fam"} onClick={() => setFamily("drums")}>Drum kits</button>
      </div>
      <ul className="presets">
        {shown.map((p) => (
          <li key={`${p.font}-${p.bank}-${p.program}`}>
            <button
              className={isCurrent(p) ? "preset current" : "preset"}
              onClick={() => use(p)}
              onContextMenu={(e) =>
                openMenu(e, [
                  { label: "Play on the pads", onClick: () => use(p) },
                  { label: "Preview", hint: "doesn't change the pads", onClick: () => invoke("preview_sound", { font: p.font, bank: p.bank, program: p.program }) },
                  "-",
                  { label: "Edit, then Save to My sounds", onClick: () => use(p) },
                ])
              }
            >
              <span className="preset-name">{p.name}</span>
              <span className="preset-tag">{tag(p)}</span>
            </button>
          </li>
        ))}
        {!shown.length && <li className="muted empty">No sounds match.</li>}
      </ul>
    </>
  );
}

function NowPlaying({ status, dev, live, sampler, drums, studio }: {
  status: Status | null; dev: DeviceState; live: boolean; sampler: boolean; drums: boolean; studio: StudioView | null;
}) {
  const main = drums ? status?.drumKit : status?.sound;
  const other = drums ? status?.sound : status?.drumKit;
  const oct = dev.oc;
  return (
    <div className="now">
      <div className="now-main">
        <span className="eyebrow">Now playing</span>
        {sampler ? (
          <>
            <h1>{studio?.kitName ?? "Kit"}</h1>
            <span className="muted">Your sampler kit · {studio?.pads.filter(Boolean).length ?? 0}/16 pads</span>
          </>
        ) : (
          <>
            <h1>{main?.name ?? "—"}</h1>
            <span className="muted">{drums ? "Instrument" : "Drum kit"}: {other?.name ?? "—"}</span>
          </>
        )}
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

function RecordToggle({ studio, transport }: { studio: StudioView | null; transport: Transport | null }) {
  const armed = !!studio?.armed;
  const level = Math.min(1, (transport?.mic ?? 0) * 1.6);
  return (
    <div className="rec-toggle">
      {armed && <div className="mic-meter" title="Mic level"><div style={{ width: `${level * 100}%` }} /></div>}
      <button className={armed ? "btn small rec-on" : "btn small"} onClick={() => call("studio_arm", { on: !armed })}
        title={armed ? "Cancel recording" : "Record your mic onto a pad"}>
        <span className="rec-dot" /> {armed ? "Hold a pad…" : "Record"}
      </button>
    </div>
  );
}

function PadGrid({ dev, pads, setPads, setNotes, studio, transport }: {
  dev: DeviceState; pads: Set<number>; setPads: SetPads;
  setNotes: React.Dispatch<React.SetStateAction<Set<number>>>; studio: StudioView | null; transport: Transport | null;
}) {
  const openMenu = useMenu();
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
  const tap = (pad: number) => { press(pad); setTimeout(() => release(pad), 350); };

  return (
    <div className={`pad-card ${inMenu ? "menu-mode" : ""}`}>
      <div className="pad-card-head">
        <h3>{inMenu ? `Pad menu · ${MENU_PAGES[dev.pg] ?? ""}` : "Pads"}</h3>
        {inMenu ? <span className="muted">browsing on the pad</span> : <RecordToggle studio={studio} transport={transport} />}
      </div>
      <div className="pad-area">
        <div className="pad-grid">
          {PAD_ROWS.flat().map((pad) => {
            const label = padLabel(dev, pad);
            return (
              <button
                key={pad}
                className={`pad ${pads.has(pad) ? "down" : ""} ${label.role ?? ""}`}
                onPointerDown={(e) => { if (e.button !== 0) return; e.currentTarget.setPointerCapture(e.pointerId); press(pad); }}
                onPointerUp={() => release(pad)}
                onPointerCancel={() => release(pad)}
                onContextMenu={(e) => openMenu(e, [
                  { label: "Play", disabled: inMenu, onClick: () => tap(pad) },
                  { label: "Record my voice onto pad " + (pad + 1), onClick: () => call("studio_record_into", { pad }) },
                ])}
              >
                <span className="pad-num">{pad + 1}</span>
                <span className="pad-main">{label.main}</span>
                {label.sub && <span className="pad-sub">{label.sub}</span>}
              </button>
            );
          })}
        </div>
      </div>
    </div>
  );
}

function SamplerGrid({ studio, transport, pads, setPads, selected, setSelected }: {
  studio: StudioView | null; transport: Transport | null; pads: Set<number>; setPads: SetPads;
  selected: number; setSelected: (p: number) => void;
}) {
  const openMenu = useMenu();
  const armed = !!studio?.armed;
  const press = (pad: number, down: boolean) => {
    if (down) setSelected(pad);
    setPads((s) => toggle(s, pad, down));
    invoke("studio_pad", { pad, down });
  };
  const tap = (pad: number) => { press(pad, true); setTimeout(() => press(pad, false), 400); };
  const firstEmpty = (from: number) => studio?.pads.findIndex((p, i) => !p && i !== from) ?? -1;

  const padMenu = (e: React.MouseEvent, pad: number, s: PadView | null | undefined) => {
    setSelected(pad);
    const empty = firstEmpty(pad);
    openMenu(e, s ? [
      { label: "Play", onClick: () => tap(pad) },
      { label: "Record over this pad", onClick: () => call("studio_record_into", { pad }) },
      "-",
      { label: "One-shot", checked: s.mode === "oneshot", onClick: () => updatePad(pad, s, { mode: "oneshot" }) },
      { label: "Hold", checked: s.mode === "hold", onClick: () => updatePad(pad, s, { mode: "hold" }) },
      { label: "Loop while held", checked: s.mode === "loop", onClick: () => updatePad(pad, s, { mode: "loop" }) },
      { label: "Reverse", checked: s.reverse, onClick: () => updatePad(pad, s, { reverse: !s.reverse }) },
      { label: "Robot voice", checked: s.fx === "robot", onClick: () => updatePad(pad, s, { fx: s.fx === "robot" ? "none" : "robot" }) },
      "-",
      { label: empty >= 0 ? `Copy to pad ${empty + 1}` : "Copy (no empty pad)", disabled: empty < 0, onClick: () => call("studio_copy_pad", { from: pad, to: empty }) },
      { label: "Spread across all pads", onClick: () => call("studio_spread", { pad }) },
      "-",
      { label: "Clear pad", danger: true, hint: "Ctrl+Z undoes", onClick: () => call("studio_clear_pad", { pad }) },
    ] : [
      { label: "Record onto this pad", onClick: () => call("studio_record_into", { pad }) },
    ]);
  };

  return (
    <div className={`pad-card ${armed ? "armed" : ""}`}>
      <div className="pad-card-head">
        <h3>{armed ? "Hold a pad and make a noise" : `Kit · ${studio?.kitName ?? ""}`}</h3>
        <div className="row">
          {!armed && <UndoRedo studio={studio} />}
          <RecordToggle studio={studio} transport={transport} />
        </div>
      </div>
      <div className="pad-area">
        <div className="pad-grid">
          {PAD_ROWS.flat().map((pad) => {
            const s = studio?.pads[pad];
            const rec = studio?.recordingPad === pad;
            return (
              <button
                key={pad}
                className={`pad ${pads.has(pad) ? "down" : ""} ${rec ? "recording" : ""} ${s ? "" : "empty"} ${selected === pad ? "selected-pad" : ""}`}
                onPointerDown={(e) => { if (e.button !== 0) return; e.currentTarget.setPointerCapture(e.pointerId); press(pad, true); }}
                onPointerUp={(e) => e.button === 0 && press(pad, false)}
                onPointerCancel={() => press(pad, false)}
                onContextMenu={(e) => padMenu(e, pad, s)}
              >
                <span className="pad-num">{pad + 1}</span>
                {rec ? (
                  <span className="pad-main">● REC</span>
                ) : s ? (
                  <>
                    <Wave peaks={s.peaks} reverse={s.reverse} />
                    <span className="pad-sub pad-sample-name">{s.name}</span>
                  </>
                ) : (
                  <span className="pad-sub">{armed ? "hold to record" : "empty"}</span>
                )}
                {s && (s.mode !== "oneshot" || s.fx !== "none" || s.echo > 0) && (
                  <span className="pad-badges">
                    {s.mode !== "oneshot" && <i>{s.mode}</i>}
                    {s.fx !== "none" && <i>{s.fx === "crush" ? "lo-fi" : s.fx}</i>}
                    {s.echo > 0 && <i>echo</i>}
                  </span>
                )}
              </button>
            );
          })}
        </div>
      </div>
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
          return <div key={n} className={`key black ${notes.has(n) ? "on" : ""}`} style={{ left: `${leftWhites * w - w * 0.3}%`, width: `${w * 0.6}%` }} />;
        })}
    </div>
  );
}
