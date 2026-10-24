// Sampler kits, My sounds, pad editor and the looper bar (backend: studio.rs).
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { useMenu } from "./menu";

export type PlayMode = "oneshot" | "hold" | "loop";
export type VoiceFx = "none" | "robot" | "crush";
export interface PadSample {
  name: string; file: string; mode: PlayMode; gain: number; pitch: number; reverse: boolean; echo: number; fx: VoiceFx;
}
export interface PadView extends PadSample { seconds: number; peaks: number[] }
export interface SoundParams { reverb: number; chorus: number; brightness: number; volume: number; pan: number; octave: number; echo: number }
export interface MySound { id: string; name: string; fontName: string; bank: number; program: number; params: SoundParams }
export interface StudioView {
  kitId: string;
  kitName: string;
  kits: { id: string; name: string }[];
  sounds: MySound[];
  pads: (PadView | null)[];
  active: boolean;
  armed: boolean;
  recordingPad: number | null;
  canUndo: boolean;
  canRedo: boolean;
  bpm: number;
  bars: number;
  metronome: boolean;
  layerMutes: boolean[];
  loopRedo: number;
  sessionFile: string | null;
  exportsDir: string;
}
export interface Transport { pos: number; seconds: number; playing: boolean; recording: boolean; countinBeats: number; mic: number }

export function useStudio() {
  const [studio, setStudio] = useState<StudioView | null>(null);
  const [transport, setTransport] = useState<Transport | null>(null);
  useEffect(() => {
    invoke<StudioView>("get_studio").then(setStudio).catch(() => {});
    const un = [
      listen<StudioView>("studio", (e) => setStudio(e.payload)),
      listen<Transport>("transport", (e) => setTransport(e.payload)),
    ];
    return () => un.forEach((p) => p.then((f) => f()));
  }, []);
  return { studio, transport };
}

/** Run a backend call and surface its error (they're all user-facing). */
export function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T | undefined> {
  return invoke<T>(cmd, args).catch((e) => {
    alert(String(e));
    return undefined;
  });
}

/** A name that turns into a text field while renaming. */
function Editable({ value, editing, onDone }: { value: string; editing: boolean; onDone: (v: string | null) => void }) {
  const [v, setV] = useState(value);
  useEffect(() => setV(value), [value, editing]);
  if (!editing) return <span className="preset-name">{value}</span>;
  return (
    <input
      className="rename"
      autoFocus
      value={v}
      onChange={(e) => setV(e.target.value)}
      onClick={(e) => e.stopPropagation()}
      onBlur={() => onDone(v.trim() || null)}
      onKeyDown={(e) => {
        if (e.key === "Enter") onDone(v.trim() || null);
        if (e.key === "Escape") onDone(null);
      }}
    />
  );
}

export function KitsPanel({ studio, currentSound }: { studio: StudioView | null; currentSound: string | null }) {
  const openMenu = useMenu();
  const [renaming, setRenaming] = useState<string | null>(null);
  const [naming, setNaming] = useState<string | null>(null);
  if (!studio) return <p className="muted empty">Sampler isn't running.</p>;
  const filled = studio.pads.filter(Boolean).length;

  const loadKit = (id: string) => call("kit_load", { id }).then(() => call("studio_activate"));
  const kitMenu = (e: React.MouseEvent, k: { id: string; name: string }) =>
    openMenu(e, [
      { label: "Play this kit on the pads", onClick: () => loadKit(k.id) },
      { label: "Rename", onClick: () => setRenaming(k.id) },
      { label: "Duplicate", onClick: () => call("kit_duplicate", { id: k.id }) },
      "-",
      {
        label: "Delete kit…", danger: true,
        onClick: () => confirm(`Delete "${k.name}" and all its recordings? This can't be undone.`) && call("kit_delete", { id: k.id }),
      },
    ]);
  const soundMenu = (e: React.MouseEvent, s: MySound) =>
    openMenu(e, [
      { label: "Play on the pads", onClick: () => call("load_my_sound", { id: s.id }) },
      { label: "Rename", onClick: () => setRenaming(s.id) },
      "-",
      { label: "Delete sound…", danger: true, onClick: () => confirm(`Delete "${s.name}"?`) && call("delete_my_sound", { id: s.id }) },
    ]);

  return (
    <div className="kits">
      <div className="kit-current" onContextMenu={(e) => kitMenu(e, { id: studio.kitId, name: studio.kitName })}>
        <input
          className="kit-name"
          value={studio.kitName}
          onChange={(e) => call("kit_rename", { id: studio.kitId, name: e.target.value })}
          aria-label="Kit name"
        />
        <span className="muted">{filled}/16 pads recorded</span>
        <button className={studio.active ? "btn primary on" : "btn primary"} onClick={() => call("studio_activate")}>
          {studio.active ? "Playing this kit on the pads" : "Play this kit on the pads"}
        </button>
      </div>

      <div className="kits-scroll">
        <div className="kits-head">
          <h3>Kits</h3>
          {naming === null ? (
            <button className="btn ghost small" onClick={() => setNaming("")}>+ New</button>
          ) : (
            <form
              className="kit-new"
              onSubmit={(e) => {
                e.preventDefault();
                if (naming.trim()) call("kit_new", { name: naming.trim() }).then(() => call("studio_activate"));
                setNaming(null);
              }}
            >
              <input autoFocus placeholder="Kit name" value={naming} onChange={(e) => setNaming(e.target.value)} onBlur={() => !naming && setNaming(null)} />
              <button className="btn primary small">Create</button>
            </form>
          )}
        </div>
        <ul className="presets">
          {studio.kits.map((k) => (
            <li key={k.id}>
              <button className={k.id === studio.kitId ? "preset current" : "preset"} onClick={() => loadKit(k.id)} onContextMenu={(e) => kitMenu(e, k)}>
                <Editable value={k.name} editing={renaming === k.id} onDone={(v) => { setRenaming(null); if (v) call("kit_rename", { id: k.id, name: v }); }} />
              </button>
            </li>
          ))}
        </ul>

        <div className="kits-head">
          <h3>My sounds</h3>
        </div>
        <ul className="presets">
          {studio.sounds.map((s) => (
            <li key={s.id}>
              <button
                className={currentSound === s.id ? "preset current" : "preset"}
                onClick={() => call("load_my_sound", { id: s.id })}
                onContextMenu={(e) => soundMenu(e, s)}
              >
                <Editable value={s.name} editing={renaming === s.id} onDone={(v) => { setRenaming(null); if (v) call("rename_my_sound", { id: s.id, name: v }); }} />
                <span className="preset-tag">{s.bank === 128 ? "kit" : "sound"}</span>
              </button>
            </li>
          ))}
          {!studio.sounds.length && (
            <li className="muted empty small-text">Tweak any sound in the editor on the right, then press <b>Save to My sounds</b>.</li>
          )}
        </ul>
      </div>
    </div>
  );
}

const MODES: { id: PlayMode; label: string }[] = [
  { id: "oneshot", label: "One-shot" },
  { id: "hold", label: "Hold" },
  { id: "loop", label: "Loop" },
];
const FXS: { id: VoiceFx; label: string }[] = [
  { id: "none", label: "Clean" },
  { id: "robot", label: "Robot" },
  { id: "crush", label: "Lo-fi" },
];

export function updatePad(pad: number, sample: PadView, patch: Partial<PadSample>) {
  const { seconds: _s, peaks: _p, ...base } = sample;
  return call("studio_update_pad", { pad, sample: { ...base, ...patch } });
}

export function PadEditor({ pad, sample, studio }: { pad: number; sample: PadView | null; studio: StudioView | null }) {
  return (
    <div className="card editor">
      <div className="card-head">
        <h3>Pad {pad + 1}</h3>
        <UndoRedo studio={studio} />
      </div>
      {!sample ? (
        <p className="muted small-text">Empty. Press <b>Record</b>, then hold this pad and make a sound.</p>
      ) : (
        <PadControls pad={pad} sample={sample} />
      )}
    </div>
  );
}

function PadControls({ pad, sample }: { pad: number; sample: PadView }) {
  const update = (patch: Partial<PadSample>) => updatePad(pad, sample, patch);
  return (
    <>
      <Wave peaks={sample.peaks} reverse={sample.reverse} big />
      <div className="row">
        <input className="field grow" value={sample.name} onChange={(e) => update({ name: e.target.value })} aria-label="Pad name" />
        <span className="muted small-text">{sample.seconds.toFixed(1)}s</span>
      </div>
      <Seg label="Play" options={MODES} value={sample.mode} onChange={(mode) => update({ mode })} />
      <Seg label="Voice" options={FXS} value={sample.fx} onChange={(fx) => update({ fx })} />
      <Slider label="Echo" min={0} max={1} step={0.05} value={sample.echo} show={`${Math.round(sample.echo * 100)}%`} onChange={(echo) => update({ echo })} />
      <Slider label="Pitch" min={-24} max={24} step={1} value={sample.pitch} show={signed(sample.pitch)} onChange={(pitch) => update({ pitch })} />
      <Slider label="Level" min={0} max={2} step={0.05} value={sample.gain} show={`${Math.round(sample.gain * 100)}%`} onChange={(gain) => update({ gain })} />
      <div className="editor-actions">
        <button className="btn small" onClick={() => update({ pitch: 12 })}>Chipmunk</button>
        <button className="btn small" onClick={() => update({ pitch: -12 })}>Monster</button>
        <button className={sample.reverse ? "btn small on" : "btn small"} onClick={() => update({ reverse: !sample.reverse })}>Reverse</button>
        <button className="btn small" title="Copy to all 16 pads, pitched up a major scale" onClick={() => call("studio_spread", { pad })}>Spread</button>
      </div>
    </>
  );
}

export function UndoRedo({ studio }: { studio: StudioView | null }) {
  return (
    <div className="undo-redo">
      <button className="btn small" disabled={!studio?.canUndo} title="Undo (Ctrl+Z)" onClick={() => call("studio_undo", { redo: false })}>↶ Undo</button>
      <button className="btn small" disabled={!studio?.canRedo} title="Redo (Ctrl+Y)" onClick={() => call("studio_undo", { redo: true })}>↷ Redo</button>
    </div>
  );
}

export function Seg<T extends string>({ label, options, value, onChange }: {
  label: string; options: { id: T; label: string }[]; value: T; onChange: (v: T) => void;
}) {
  return (
    <div className="labeled">
      <span>{label}</span>
      <div className="seg">
        {options.map((m) => (
          <button key={m.id} className={value === m.id ? "on" : ""} onClick={() => onChange(m.id)}>{m.label}</button>
        ))}
      </div>
    </div>
  );
}

export function Slider({ label, min, max, step, value, show, onChange }: {
  label: string; min: number; max: number; step: number; value: number; show: string; onChange: (v: number) => void;
}) {
  return (
    <label className="slider">
      <span>{label}</span>
      <input type="range" min={min} max={max} step={step} value={value} onChange={(e) => onChange(Number(e.target.value))} />
      <b>{show}</b>
    </label>
  );
}

export const signed = (n: number) => (n > 0 ? `+${n}` : `${n}`);

export function Wave({ peaks, reverse, big }: { peaks: number[]; reverse?: boolean; big?: boolean }) {
  const p = reverse ? [...peaks].reverse() : peaks;
  return (
    <div className={big ? "wave big" : "wave"} aria-hidden>
      {p.map((v, i) => <i key={i} style={{ height: `${Math.max(4, Math.min(1, v) * 100)}%` }} />)}
    </div>
  );
}

export function LooperBar({ studio, transport, status }: { studio: StudioView | null; transport: Transport | null; status: string }) {
  const openMenu = useMenu();
  const [saved, setSaved] = useState<string | null>(null);
  if (!studio) return null;
  const t = transport;
  const layers = studio.layerMutes;
  const hasLoop = layers.length > 0;
  const busy = !!(t?.recording || t?.countinBeats);
  const tempo = (patch: Partial<{ bpm: number; bars: number; metronome: boolean }>) =>
    call("looper_tempo", { bpm: studio.bpm, bars: studio.bars, metronome: studio.metronome, ...patch });
  const loop = (cmd: string, layer?: number) => call("looper_cmd", { cmd, layer });
  const exportLoop = () => call<string>("export_loop").then((p) => p && setSaved(p));

  let state = "Press Loop: one bar of clicks, then it records";
  if (t?.countinBeats) state = `Count-in… ${t.countinBeats}`;
  else if (t?.recording) state = hasLoop ? "Overdubbing a layer…" : "Recording loop…";
  else if (hasLoop) state = t?.playing ? `Looping · ${layers.length} layer${layers.length === 1 ? "" : "s"}` : "Loop stopped";

  const loopMenu = (e: React.MouseEvent) =>
    openMenu(e, [
      { label: hasLoop ? "Overdub a layer" : "Record a loop", onClick: () => call("looper_record") },
      { label: t?.playing ? "Stop" : "Play", disabled: !hasLoop, onClick: () => loop("stop") },
      "-",
      { label: "Undo last layer", disabled: !hasLoop && !busy, onClick: () => loop("undo") },
      { label: "Redo layer", disabled: !studio.loopRedo, onClick: () => loop("redo") },
      { label: "Save loop as WAV", disabled: !hasLoop, onClick: exportLoop },
      "-",
      { label: "Clear loop", danger: true, disabled: !hasLoop, onClick: () => loop("clear") },
    ]);
  const layerMenu = (e: React.MouseEvent, i: number, muted: boolean) =>
    openMenu(e, [
      { label: muted ? "Unmute" : "Mute", onClick: () => call("looper_mute", { layer: i, muted: !muted }) },
      { label: "Solo", onClick: () => call("looper_solo", { layer: i }) },
      "-",
      { label: `Delete layer L${i + 1}`, danger: true, hint: "Redo brings it back", onClick: () => loop("remove", i) },
    ]);

  return (
    <div className="looper" onContextMenu={loopMenu}>
      <button className={`rec-btn ${busy ? "live" : ""}`} onClick={() => call("looper_record")}
        title={hasLoop ? "Overdub one more layer" : "Record a new loop (one bar count-in)"}>
        <span className="rec-dot" /> {hasLoop ? "Overdub" : "Loop"}
      </button>
      <div className="btn-group">
        <button className="btn" disabled={!hasLoop} onClick={() => loop("stop")}>{t?.playing ? "■" : "▶"}</button>
        <button className="btn" disabled={!hasLoop && !busy} title="Undo last layer" onClick={() => loop("undo")}>↶</button>
        <button className="btn" disabled={!studio.loopRedo} title="Redo layer" onClick={() => loop("redo")}>↷</button>
      </div>

      <div className="looper-track">
        <div className="looper-status">
          <span>{state}</span>
          <span className="layers">
            {layers.map((muted, i) => (
              <button key={i} className={muted ? "layer muted" : "layer"} title="Click to mute · right-click for more"
                onClick={() => call("looper_mute", { layer: i, muted: !muted })}
                onContextMenu={(e) => layerMenu(e, i, muted)}>
                L{i + 1}
              </button>
            ))}
          </span>
        </div>
        <div className="progress">
          <div className={`progress-fill ${t?.recording ? "rec" : ""}`} style={{ width: `${(t?.pos ?? 0) * 100}%` }} />
          {Array.from({ length: studio.bars * 4 }, (_, i) => (
            <span key={i} className={i % 4 === 0 ? "tick bar" : "tick"} style={{ left: `${(i / (studio.bars * 4)) * 100}%` }} />
          ))}
        </div>
      </div>

      <div className="tempo">
        <label title="Tempo">
          <input type="number" min={40} max={220} value={studio.bpm} disabled={hasLoop || busy}
            onChange={(e) => tempo({ bpm: Number(e.target.value) || 100 })} />
          <span>BPM</span>
        </label>
        <label title="Loop length">
          <select value={studio.bars} disabled={hasLoop || busy} onChange={(e) => tempo({ bars: Number(e.target.value) })}>
            {[1, 2, 4, 8].map((b) => <option key={b} value={b}>{b}</option>)}
          </select>
          <span>bars</span>
        </label>
        <label className="check-inline" title="Metronome click while looping (never recorded)">
          <input type="checkbox" checked={studio.metronome} onChange={(e) => tempo({ metronome: e.target.checked })} /> Click
        </label>
      </div>

      <div className="looper-save">
        <button className="btn small" disabled={!hasLoop} onClick={exportLoop}>Save loop</button>
        <button className={studio.sessionFile ? "btn small rec-on" : "btn small"}
          onClick={() => call<string | null>("toggle_session").then((p) => p && setSaved(p))}>
          {studio.sessionFile ? "■ Stop rec" : "● Rec session"}
        </button>
        <button className="link" title={studio.exportsDir} onClick={() => revealItemInDir(saved ?? studio.exportsDir)}>Folder</button>
      </div>
      <div className="statusline" title={status}>{studio.sessionFile ? "Recording everything you play…" : saved ? `Saved ${baseName(saved)}` : status}</div>
    </div>
  );
}

const baseName = (p: string) => p.split(/[\\/]/).pop();
