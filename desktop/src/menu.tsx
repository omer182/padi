// App-wide right-click menu. Anything can call `openMenu(e, items)` from an
// onContextMenu handler; the browser's own menu is suppressed elsewhere.
import { createContext, ReactNode, useCallback, useContext, useEffect, useLayoutEffect, useRef, useState } from "react";

export type MenuItem =
  | { label: string; onClick: () => void; checked?: boolean; danger?: boolean; disabled?: boolean; hint?: string }
  | "-";

type Open = (e: React.MouseEvent, items: MenuItem[]) => void;
const Ctx = createContext<Open>(() => {});
export const useMenu = () => useContext(Ctx);

export function MenuProvider({ children }: { children: ReactNode }) {
  const [menu, setMenu] = useState<{ x: number; y: number; items: MenuItem[] } | null>(null);
  const ref = useRef<HTMLDivElement>(null);

  const open = useCallback<Open>((e, items) => {
    e.preventDefault();
    e.stopPropagation();
    setMenu({ x: e.clientX, y: e.clientY, items });
  }, []);

  useEffect(() => {
    // No browser menu ("Reload", "Inspect") anywhere except text fields.
    const block = (e: MouseEvent) => {
      const t = e.target as HTMLElement;
      if (!t.closest("input, textarea")) e.preventDefault();
    };
    const close = () => setMenu(null);
    const key = (e: KeyboardEvent) => e.key === "Escape" && close();
    window.addEventListener("contextmenu", block);
    window.addEventListener("blur", close);
    window.addEventListener("resize", close);
    window.addEventListener("keydown", key);
    return () => {
      window.removeEventListener("contextmenu", block);
      window.removeEventListener("blur", close);
      window.removeEventListener("resize", close);
      window.removeEventListener("keydown", key);
    };
  }, []);

  // Keep the menu on screen.
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el || !menu) return;
    const r = el.getBoundingClientRect();
    const x = Math.min(menu.x, window.innerWidth - r.width - 6);
    const y = Math.min(menu.y, window.innerHeight - r.height - 6);
    el.style.left = `${Math.max(6, x)}px`;
    el.style.top = `${Math.max(6, y)}px`;
  }, [menu]);

  return (
    <Ctx.Provider value={open}>
      {children}
      {menu && (
        <div className="ctx-backdrop" onPointerDown={() => setMenu(null)} onContextMenu={(e) => { e.preventDefault(); setMenu(null); }}>
          <div ref={ref} className="ctx-menu" role="menu" style={{ left: menu.x, top: menu.y }} onPointerDown={(e) => e.stopPropagation()}>
            {menu.items.map((it, i) =>
              it === "-" ? (
                <div key={i} className="ctx-sep" />
              ) : (
                <button
                  key={i}
                  role="menuitem"
                  className={`ctx-item ${it.danger ? "danger" : ""}`}
                  disabled={it.disabled}
                  onClick={() => { setMenu(null); it.onClick(); }}
                >
                  <span className="ctx-check">{it.checked ? "✓" : ""}</span>
                  <span>{it.label}</span>
                  {it.hint && <span className="ctx-hint">{it.hint}</span>}
                </button>
              ),
            )}
          </div>
        </div>
      )}
    </Ctx.Provider>
  );
}
