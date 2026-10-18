import { useEffect, useRef, useState } from "react";

export interface PaletteAction {
  id: string;
  title: string;
  note: string;
  run: () => void;
}

/** Ctrl+K: type to filter nodes and templates, Enter or a click runs the match. */
export default function CommandPalette({ actions, onClose }: { actions: PaletteAction[]; onClose: () => void }) {
  const [q, setQ] = useState("");
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => {
    input.current?.focus();
  }, []);
  const needle = q.trim().toLowerCase();
  const items = (needle ? actions.filter((a) => `${a.title} ${a.note}`.toLowerCase().includes(needle)) : actions).slice(0, 14);
  return (
    <div
      className="cmdpal-back"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div className="cmdpal" role="dialog" aria-label="Command palette">
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
              <b>{a.title}</b>
              <small>{a.note}</small>
            </button>
          ))}
          {items.length === 0 && <p className="mut">No matches.</p>}
        </div>
      </div>
    </div>
  );
}
