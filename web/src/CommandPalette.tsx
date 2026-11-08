import { useEffect, useRef, useState } from "react";
import useDialogFocus from './useDialogFocus'
import CatalogueLabel from './CatalogueLabel'

export interface PaletteAction {
  id: string;
  title: string;
  note: string;
  experimental?: boolean;
  run: () => void;
}

export function paletteItems(actions: PaletteAction[], query: string): PaletteAction[] {
  const needle = query.trim().toLowerCase()
  return (needle ? actions.filter((a) => `${a.title} ${a.note}${a.experimental ? ' Experimental' : ''}`.toLowerCase().includes(needle)) : actions).slice(0, 14)
}

/** Ctrl+K: type to filter nodes and templates, Enter or a click runs the match. */
export default function CommandPalette({ actions, onClose }: { actions: PaletteAction[]; onClose: () => void }) {
  const [q, setQ] = useState("");
  const input = useRef<HTMLInputElement>(null);
  const dialog = useRef<HTMLDivElement>(null)
  useDialogFocus(dialog)
  useEffect(() => {
    input.current?.focus();
  }, []);
  const items = paletteItems(actions, q);
  return (
    <div
      className="cmdpal-back"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div className="cmdpal" ref={dialog} role="dialog" aria-modal="true" aria-label="Command palette" tabIndex={-1}>
        <input
          ref={input}
          value={q}
          onChange={(e) => setQ(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && items[0]) {
              items[0].run();
              onClose();
            }
          }}
          placeholder="Type a node or template…"
          aria-label="Command palette"
        />
        <div className="cmdpal-list">
          {items.map((a) => (
            <button
              key={a.id}
              onClick={() => {
                a.run();
                onClose();
              }}
            >
              <b><CatalogueLabel type={{ label: a.title, experimental: a.experimental }} /></b>
              <small>{a.note}</small>
            </button>
          ))}
          {items.length === 0 && <p className="mut">No matches.</p>}
        </div>
      </div>
    </div>
  );
}
