// Sound editor for SoundFont sounds. Changes apply live; stock sounds are
// never changed on disk: "Save to My sounds" stores a copy in My kits.
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { call, signed, Slider, SoundParams } from "./studio";

export interface Sound { font: number; bank: number; program: number; name: string; params: SoundParams; mySound: string | null }

export const DEFAULT_PARAMS: SoundParams = { reverb: 40, chorus: 0, brightness: 127, volume: 100, pan: 64, octave: 0, echo: 0 };
const same = (a: SoundParams, b: SoundParams) => (Object.keys(a) as (keyof SoundParams)[]).every((k) => a[k] === b[k]);

export function SoundEditor({ sound, target, knob }: { sound: Sound | null; target: "sound" | "drumKit"; knob: string }) {
  const [naming, setNaming] = useState<string | null>(null);
  // Remember what the sound was when it was picked, to show "edited".
  const [base, setBase] = useState<SoundParams>(DEFAULT_PARAMS);
  const key = sound ? `${sound.font}/${sound.bank}/${sound.program}/${sound.mySound ?? ""}` : "";
  useEffect(() => { if (sound) setBase(sound.params); setNaming(null); }, [key]); // eslint-disable-line react-hooks/exhaustive-deps

  if (!sound) return <div className="card editor"><p className="muted small-text">Pick a sound on the left.</p></div>;
  const p = sound.params;
  const set = (patch: Partial<SoundParams>) => invoke("set_sound_params", { target, params: { ...p, ...patch } });
  const edited = !same(p, base);
  const save = (name: string, id: string | null) =>
    call("save_my_sound", { target, name, id }).then(() => { setBase(p); setNaming(null); });

  return (
    <div className="card editor">
      <div className="card-head">
        <h3>{target === "drumKit" ? "Drum kit" : "Sound"}</h3>
        <span className="muted small-text">{sound.mySound ? "yours" : "stock"}{edited ? " · edited" : ""}</span>
      </div>
      <div className="editor-title" title={sound.name}>{sound.name}</div>
      <Slider label="Reverb" min={0} max={127} step={1} value={p.reverb} show={pct(p.reverb)} onChange={(reverb) => set({ reverb })} />
      <Slider label="Echo" min={0} max={1} step={0.05} value={p.echo} show={`${Math.round(p.echo * 100)}%`} onChange={(echo) => set({ echo })} />
      <Slider label="Chorus" min={0} max={127} step={1} value={p.chorus} show={pct(p.chorus)} onChange={(chorus) => set({ chorus })} />
      <Slider label="Bright" min={20} max={127} step={1} value={p.brightness} show={p.brightness >= 127 ? "open" : pct(p.brightness)} onChange={(brightness) => set({ brightness })} />
      {target === "sound" && (
        <Slider label="Octave" min={-2} max={2} step={1} value={p.octave} show={signed(p.octave)} onChange={(octave) => set({ octave })} />
      )}
      <Slider label="Volume" min={0} max={127} step={1} value={p.volume} show={pct(p.volume)} onChange={(volume) => set({ volume })} />
      <Slider label="Pan" min={0} max={127} step={1} value={p.pan} show={p.pan === 64 ? "C" : p.pan < 64 ? `L${64 - p.pan}` : `R${p.pan - 64}`} onChange={(pan) => set({ pan })} />

      {naming !== null ? (
        <form className="row" onSubmit={(e) => { e.preventDefault(); if (naming.trim()) save(naming.trim(), null); }}>
          <input className="field grow" autoFocus value={naming} onChange={(e) => setNaming(e.target.value)} onKeyDown={(e) => e.key === "Escape" && setNaming(null)} />
          <button className="btn primary small">Save</button>
        </form>
      ) : (
        <div className="editor-actions">
          {sound.mySound ? (
            <>
              <button className="btn primary small" disabled={!edited} onClick={() => save(sound.name, sound.mySound)}>Save</button>
              <button className="btn small" onClick={() => setNaming(`${sound.name} 2`)}>Save as new</button>
            </>
          ) : (
            <button className="btn primary small" onClick={() => setNaming(`${sound.name} (mine)`)}>Save to My sounds</button>
          )}
          <button className="btn small" disabled={!edited} onClick={() => set(base)}>Reset</button>
        </div>
      )}
      <div className="knob-line muted small-text">Pad knob: <b>{knob}</b></div>
    </div>
  );
}

const pct = (v: number) => `${Math.round((v / 127) * 100)}%`;
