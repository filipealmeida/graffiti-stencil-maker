import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  Background,
  Controls,
  MiniMap,
  ReactFlow,
  ReactFlowProvider,
  useReactFlow,
  type Edge,
} from "@xyflow/react";
import { api, type NodeType, type Stage } from "./api";
import Icon from "./Icon";
import PanelResizer from './PanelResizer'
import Inspector from "./Inspector";
import Console from "./Console";
import Perf from "./Perf";
import Report, { useReport } from "./Report";
import History from "./History";
import Workbench from "./Workbench";
import StudioEdge from "./StudioEdge";
import StudioNodeView, { KIND_COLOR } from "./StudioNode";
import Status from "./Status";
import CommandPalette from "./CommandPalette";
import {
  stageColumn,
  WORLD_END,
  redo,
  resetHistory,
  runGraph,
  rerun,
  signature,
  STORE_KEY,
  undo,
  useStudio,
  type StudioNode,
} from "./store";

const nodeTypes = { studio: StudioNodeView };
const edgeTypes = { default: StudioEdge };
const DRAFT_DELAY = 150;

function Canvas() {
  const s = useStudio();
  const rep = useReport();
  const rf = useReactFlow();
  const [drag, setDrag] = useState(false);
  const [pname, setPname] = useState("project");
  const [menu, setMenu] = useState<Stage | "templates" | "all" | null>(null);
  const [cmdOpen, setCmdOpen] = useState(false);
  // wait for the new nodes to be measured, then bring the whole graph into view
  const fit = useCallback(() => {
    setTimeout(
      () => rf.fitView({ padding: 0.08, maxZoom: 1, duration: 250 }),
      200,
    );
  }, [rf]);
  // The pan limit follows the graph. A fixed box smaller than the fitted view made React Flow snap the view
  // (nodes slid out of sight) on the first pan after "fit view".
  const extent = useMemo<[[number, number], [number, number]]>(() => {
    let x0 = 0, y0 = 0, x1 = WORLD_END, y1 = 1200;
    for (const n of s.nodes) {
      x0 = Math.min(x0, n.position.x);
      y0 = Math.min(y0, n.position.y);
      x1 = Math.max(x1, n.position.x + (n.measured?.width ?? 300));
      y1 = Math.max(y1, n.position.y + (n.measured?.height ?? 300));
    }
    const m = 6000;
    return [[x0 - m, y0 - m], [x1 + m, y1 + m]];
  }, [s.nodes]);
  const bar = useRef<HTMLDivElement>(null);
  const tpl = useRef<HTMLDivElement>(null);
  const sliding = useRef(false);
  const hasSource = s.nodes.some((n) => n.data.type === "source");
  useEffect(() => {
    const away = (e: MouseEvent) => {
      if (
        !bar.current?.contains(e.target as Node) &&
        !tpl.current?.contains(e.target as Node)
      )
        setMenu(null);
    };
    const esc = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        // one step back each press: menus, then the preview, then the selection
        const menuOpen = !!document.querySelector(".menu, .cmdpal");
        setMenu(null);
        setCmdOpen(false);
        if (menuOpen) return;
        const st = useStudio.getState();
        if (st.bench) st.openBench(null);
        else if (
          st.selected &&
          !(e.target as HTMLElement).closest?.("input, textarea, select")
        )
          st.select(null);
        return;
      }
      const t = e.target as HTMLElement;
      if (
        t.closest?.("input, textarea, select, [contenteditable]") ||
        !(e.ctrlKey || e.metaKey)
      )
        return;
      const k = e.key.toLowerCase();
      if (k === "z" && !e.shiftKey) {
        e.preventDefault();
        undo();
      } else if ((k === "z" && e.shiftKey) || k === "y") {
        e.preventDefault();
        redo();
      } else if (k === "c") useStudio.getState().copy();
      else if (k === "v") {
        e.preventDefault();
        useStudio.getState().paste();
      } else if (k === "d") {
        e.preventDefault();
        useStudio.getState().duplicate();
      } else if (k === "enter") {
        e.preventDefault();
        rerun();
      } else if (k === "k") {
        e.preventDefault();
        setCmdOpen((o) => !o);
      }
    };
    const down = (e: PointerEvent) => {
      sliding.current =
        (e.target as HTMLElement | null)?.matches?.("input[type=range]") ??
        false;
    };
    const up = () => {
      if (sliding.current) {
        sliding.current = false;
        runGraph();
      }
    };
    window.addEventListener("mousedown", away);
    window.addEventListener("keydown", esc);
    window.addEventListener("pointerdown", down);
    window.addEventListener("pointerup", up);
    window.addEventListener("pointercancel", up);
    return () => {
      window.removeEventListener("mousedown", away);
      window.removeEventListener("keydown", esc);
      window.removeEventListener("pointerdown", down);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("pointercancel", up);
    };
  }, []);

  useEffect(() => {
    Promise.all([api.nodeTypes(), api.restoreImages()]).then(([t]) => {
      s.setTypes(t);
      try {
        const saved = JSON.parse(localStorage.getItem(STORE_KEY) ?? "null");
        if (saved) {
          s.load(saved.nodes, saved.edges);
          if (saved.nodes?.length) fit();
        }
        resetHistory();
      } catch {
        /* ignore a corrupt autosave */
      }
    });
  }, []);

  // autosave and re-run only when something that affects the result changed
  const sig = signature(s);
  const first = useRef(true);
  useEffect(() => {
    if (!Object.keys(s.types).length) return;
    try {
      localStorage.setItem(
        STORE_KEY,
        JSON.stringify({ nodes: s.nodes, edges: s.edges }),
      );
    } catch {
      useStudio.setState({
        error:
          "autosave failed (browser storage is full or blocked): use Save project to keep your work",
      });
    }
    // one run per settled edit; it evaluates node by node, so the trace stage shows up first.
    // While a slider is held down a coarse draft runs instead, and the full run follows on release.
    const wait = first.current ? 0 : sliding.current ? DRAFT_DELAY : 350;
    const full = setTimeout(() => runGraph(sliding.current), wait);
    first.current = false;
    return () => clearTimeout(full);
  }, [sig, Object.keys(s.types).length]);

  const addFile = useCallback(
    async (f: File, at?: { x: number; y: number }) => {
      try {
        const img = await api.upload(f);
        if (!useStudio.getState().nodes.length) {
          useStudio.getState().vectorStarter(img.id, img.name);
          fit();
        } else {
          // one image in the graph: the drop swaps it, so the whole pipeline re-runs on the new picture
          const sources = useStudio
            .getState()
            .nodes.filter((n) => n.data.type === "source");
          if (sources.length === 1)
            useStudio.setState((st) => ({
              nodes: st.nodes.map((n) =>
                n.id === sources[0].id
                  ? {
                      ...n,
                      data: {
                        ...n.data,
                        title: img.name,
                        params: { ...n.data.params, image_id: img.id },
                      },
                    }
                  : n,
              ),
            }));
          else
            useStudio
              .getState()
              .addNode("source", { image_id: img.id }, at, img.name);
        }
      } catch (e) {
        useStudio.setState({
          error: String(e instanceof Error ? e.message : e),
        });
      }
    },
    [fit],
  );

  const onDrop = (e: React.DragEvent) => {
    e.preventDefault();
    setDrag(false);
    const f = e.dataTransfer.files[0];
    if (f) addFile(f, rf.screenToFlowPosition({ x: e.clientX, y: e.clientY }));
  };

  /** Can an output of this kind feed an input of that kind? The same rule as drawing a connection by hand. */
  const kindFits = (out: string, inp: string) =>
    inp === out || (inp === "any" && (out === "image" || out === "mask"));

  /** The edge under a dragged, unconnected node that the node could be placed in the middle of. */
  const edgeToSplit = (nodeId: string) => {
    const st = useStudio.getState();
    if (st.edges.some((e) => e.source === nodeId || e.target === nodeId)) return null;
    const node = st.nodes.find((n) => n.id === nodeId);
    const t = node && st.types[node.data.type];
    const el = document.querySelector(`.react-flow__node[data-id="${nodeId}"]`);
    if (!node || !t || !el) return null;
    const r = el.getBoundingClientRect();
    const cx = r.x + r.width / 2, cy = r.y + r.height / 2;
    let best: { id: string; d: number; input: string; output: string } | null = null;
    for (const e of st.edges) {
      const path = document.querySelector(`.react-flow__edge[data-id="${CSS.escape(e.id)}"] path`) as SVGPathElement | null;
      const m = path?.getScreenCTM();
      if (!path || !m) continue;
      const out = st.types[st.nodes.find((n) => n.id === e.source)?.data.type ?? ""]?.outputs.find((o) => o.name === e.sourceHandle);
      const inp = st.types[st.nodes.find((n) => n.id === e.target)?.data.type ?? ""]?.inputs.find((i) => i.name === e.targetHandle);
      if (!out || !inp) continue;
      const myIn = t.inputs.find((i) => kindFits(out.type, i.type));
      const myOut = t.outputs.find((o) => kindFits(o.type, inp.type));
      if (!myIn || !myOut) continue;
      const len = path.getTotalLength();
      for (let k = 0; k <= 40; k++) {
        const q = path.getPointAtLength((len * k) / 40);
        const x = q.x * m.a + q.y * m.c + m.e, y = q.x * m.b + q.y * m.d + m.f;
        if (x < r.x || x > r.x + r.width || y < r.y || y > r.y + r.height) continue;
        const d = Math.hypot(x - cx, y - cy);
        if (!best || d < best.d) best = { id: e.id, d, input: myIn.name, output: myOut.name };
      }
    }
    return best;
  };
  const [splitEdge, setSplitEdge] = useState<string | null>(null);
  useEffect(() => {
    if (!splitEdge) return;
    const el = document.querySelector(`.react-flow__edge[data-id="${CSS.escape(splitEdge)}"]`);
    el?.classList.add("splice");
    return () => el?.classList.remove("splice");
  }, [splitEdge]);

  const isValid = (c: {
    source: string;
    sourceHandle?: string | null;
    target: string;
    targetHandle?: string | null;
  }) => {
    const nodes = useStudio.getState().nodes;
    const a = nodes.find((n) => n.id === c.source),
      b = nodes.find((n) => n.id === c.target);
    if (!a || !b || a.id === b.id) return false;
    const o = s.types[a.data.type]?.outputs.find(
      (x) => x.name === c.sourceHandle,
    );
    const i = s.types[b.data.type]?.inputs.find(
      (x) => x.name === c.targetHandle,
    );
    return (
      !!o &&
      !!i &&
      (i.type === o.type ||
        (i.type === "any" && (o.type === "image" || o.type === "mask")))
    );
  };

  const save = async () => {
    const p = await api.exportProject({ nodes: s.nodes, edges: s.edges });
    const url = URL.createObjectURL(
      new Blob([JSON.stringify(p)], { type: "application/json" }),
    );
    const a = document.createElement("a");
    a.href = url;
    a.download = `${pname || "project"}.stencil.json`;
    a.hidden = true;
    document.body.appendChild(a);
    a.click();
    a.remove();
    setTimeout(() => URL.revokeObjectURL(url), 10_000);
  };
  const open = async (f: File) => {
    try {
      const g = await api.importProject(JSON.parse(await f.text()));
      setPname(f.name.replace(/\.stencil\.json$|\.json$/, ""));
      s.load(g.nodes as StudioNode[], g.edges as Edge[]);
      fit();
    } catch (e) {
      useStudio.setState({
        error: `cannot open project: ${e instanceof Error ? e.message : e}`,
      });
    }
  };

  const addFromMenu = (t: NodeType) => {
    const c = rf.screenToFlowPosition({ x: window.innerWidth * 0.3, y: 160 });
    const sp = stageColumn(t.stage);
    const id = s.addNode(t.id, {}, { x: t.stage === "trace" ? c.x : sp.x + 30, y: c.y });
    setMenu(null);
    revealNode(id);
  };
  // once the new node is measured, pan (never zoom) just far enough to show all of it
  const revealNode = (id: string) => {
    setTimeout(() => {
      const n = rf.getNode(id);
      const box = document.querySelector(".canvas")?.getBoundingClientRect();
      if (!n || !box) return;
      const { x, y, zoom } = rf.getViewport();
      const w = (n.measured?.width ?? 260) * zoom;
      const h = (n.measured?.height ?? 160) * zoom;
      const gap = 30;
      const sx = n.position.x * zoom + x;
      const sy = n.position.y * zoom + y;
      const dx = sx < gap ? gap - sx : sx + w + gap > box.width ? box.width - gap - w - sx : 0;
      const dy = sy < gap ? gap - sy : sy + h + gap > box.height ? box.height - gap - h - sy : 0;
      if (dx || dy) rf.setViewport({ x: x + dx, y: y + dy, zoom }, { duration: 250 });
    }, 120);
  };
  // fill each column to the height the page allows, then start the next one
  const menuStyle = (n: number) => {
    const rows = Math.max(4, Math.floor((window.innerHeight - 70) / 56));
    return { "--rows": Math.min(n, rows) } as React.CSSProperties;
  };
  const templates = [
    [
      "Simple",
      "image → threshold → stencil. The quickest route to an STL.",
      s.starter,
    ],
    [
      "Colour tones",
      "image → reduce colours → tone patterns → stencil. For pictures with shades.",
      s.colourStarter,
    ],
    [
      "Vector",
      "image → threshold → trace → vector stencil. Smooth curves instead of pixels.",
      s.vectorStarter,
    ],
    [
      "Colour set",
      "image → split colours → one stencil per colour → colour set. Layers with registration holes and pins, as one ZIP.",
      s.colourSetStarter,
    ],
    [
      "Tiles, pads & screws",
      "image → threshold → stencil → split volume → pads & holes → screws. A big stencil in printable tiles, with mounting pads and joiner plates.",
      s.tiledStarter,
    ],
    [
      "Tiles first",
      "image → threshold → tile layout → stencil (per tile) → pads & holes → screws. Cut into tiles before bridging, so every tile is island-free by itself.",
      s.tilesFirstStarter,
    ],
  ] as const;
  const runTemplate = (make: (imageId: string, title: string) => void) => {
    const src = s.nodes.find((n) => n.data.type === "source");
    if (src) {
      make(String(src.data.params.image_id), String(src.data.title ?? "image"));
      fit();
      setMenu(null);
      setCmdOpen(false);
    }
  };
  const current = Object.values(s.types).filter((t) => !t.legacy);
  const popular = current.sort(
    (a, b) =>
      (s.usage[b.id] ?? 0) - (s.usage[a.id] ?? 0) ||
      a.label.localeCompare(b.label),
  );
  const stageNodes = (st: Stage) =>
    current.filter((t) => t.stage === st);
  const cmdActions = useMemo(
    () => [
      ...current.map((t) => ({
        id: `node:${t.id}`,
        title: t.label,
        note: `${t.stage} · ${t.doc.split(/(?<=\.)\s/)[0]}${FORMER_NAMES[t.id] ? ` · was ${FORMER_NAMES[t.id]}` : ""}`,
        run: () => addFromMenu(t),
      })),
      ...templates.map(([name, doc, make]) => ({
        id: `tpl:${name}`,
        title: `${name} template`,
        note: doc,
        run: () => runTemplate(make),
      })),
    ],
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [s.types, s.nodes],
  );
  return (
    <div
      className="app"
      onDragOver={(e) => {
        e.preventDefault();
        setDrag(true);
      }}
      onDragLeave={() => setDrag(false)}
      onDrop={onDrop}
    >
      <header>
        <b>Stencil studio</b>
        <div className="palette" ref={bar}>
          {(["trace", "stencil", "post"] as Stage[]).map((st) => (
            <div key={st} className="pal">
              <button
                className={`stage-${st}${menu === st ? " on" : ""}`}
                aria-haspopup="true"
                aria-expanded={menu === st}
                onClick={() => setMenu(menu === st ? null : st)}
              >
                + {st}
              </button>
              {menu === st && (
                <div className="menu" style={menuStyle(stageNodes(st).length)}>
                  {stageNodes(st).map((t) => (
                    <button
                      key={t.id}
                      title={t.doc}
                      onClick={() => addFromMenu(t)}
                    >
                      <b>{t.label}</b>
                      <small>{t.doc.split(/(?<=\.)\s/)[0]}</small>
                    </button>
                  ))}
                </div>
              )}
            </div>
          ))}
          <div className="pal">
            <button
              className={menu === "all" ? "on" : ""}
              aria-haspopup="true"
              aria-expanded={menu === "all"}
              onClick={() => setMenu(menu === "all" ? null : "all")}
              title="Every node, most used first"
            >
              + All
            </button>
            {menu === "all" && (
              <div className="menu" style={menuStyle(popular.length)}>
                {popular.map((t) => {
                  const n = s.usage[t.id] ?? 0;
                  return (
                    <button
                      key={t.id}
                      className={`stage-${t.stage}`}
                      title={`${t.doc}\n\n${n ? `Added ${n} time${n === 1 ? "" : "s"} in this browser` : "Not used yet"}`}
                      onClick={() => addFromMenu(t)}
                    >
                      <b>
                        {t.label}
                        {n > 0 && <span className="cnt">{n}</span>}
                      </b>
                      <small>
                        {t.stage} · {t.doc.split(/(?<=\.)\s/)[0]}
                      </small>
                    </button>
                  );
                })}
              </div>
            )}
          </div>
        </div>
        <label className="btn icon-btn" title="Open image…">
          <Icon name="image" />
          <span className="sr">Open image…</span>
          <input
            type="file"
            accept=".png,.jpg,.jpeg,.bmp,.svg"
            hidden
            onChange={(e) => e.target.files?.[0] && addFile(e.target.files[0])}
          />
        </label>
        <div className="pal" ref={tpl}>
          <button
            className={menu === "templates" ? "on" : ""}
            aria-haspopup="true"
            aria-expanded={menu === "templates"}
            onClick={() => setMenu(menu === "templates" ? null : "templates")}
            title="Templates: rebuild the graph around the current picture"
            aria-label="Templates"
          >
            <Icon name="templates" />
          </button>
          {menu === "templates" && (
            <div className="menu wide">
              {templates.map(([name, doc, make]) => (
                <button
                  key={name}
                  disabled={!hasSource}
                  onClick={() => runTemplate(make)}
                >
                  <b>{name}</b>
                  <small>{doc}</small>
                </button>
              ))}
              <small className="menu-note">
                {hasSource
                  ? "Replaces the current graph (Ctrl+Z undoes it)."
                  : "Open an image first."}
              </small>
            </div>
          )}
        </div>
        <button
          onClick={() => setCmdOpen(true)}
          title="Command palette (Ctrl+K)"
          aria-label="Command palette"
        >
          <Icon name="command" />
        </button>
        <History />
        <button title="Undo (Ctrl+Z)" aria-label="Undo" disabled={!s.canUndo} onClick={undo}>
          <Icon name="undo" />
        </button>
        <button
          title="Redo (Ctrl+Shift+Z)"
          aria-label="Redo"
          disabled={!s.canRedo}
          onClick={redo}
        >
          <Icon name="redo" />
        </button>
        <span className="spacer" />
        <label className="btn icon-btn" title="Open project…">
          <Icon name="folder" />
          <span className="sr">Open project…</span>
          <input
            type="file"
            accept=".json"
            hidden
            onChange={(e) => {
              const f = e.target.files?.[0];
              e.target.value = "";
              if (f) open(f);
            }}
          />
        </label>
        <input
          className="pname"
          value={pname}
          onChange={(e) => setPname(e.target.value)}
          aria-label="Project name"
        />
        <button onClick={save} title="Save project" aria-label="Save project">
          <Icon name="save" />
        </button>
        <button
          title="New project: discard the current graph"
          aria-label="New"
          onClick={() => {
            if (
              s.nodes.length &&
              !window.confirm(
                "Discard the current graph? Save the project first to keep it.",
              )
            )
              return;
            s.load([], []);
            localStorage.removeItem(STORE_KEY);
          }}
        >
          <Icon name="new" />
        </button>
        <button
          onClick={rerun}
          disabled={s.busy}
          title="Rerun the whole graph (Ctrl+Enter). Unchanged nodes come from the cache."
          aria-label="Rerun"
        >
          <Icon name="rerun" />
        </button>
        {s.busy && s.progress.total > 0 && (
          <span
            className="gbar"
            role="progressbar"
            aria-label="Run progress"
            aria-valuemin={0}
            aria-valuemax={s.progress.total}
            aria-valuenow={Math.floor(s.progress.done)}
            title={`${Math.floor(s.progress.done)}/${s.progress.total} nodes`}
          >
            <i
              style={{
                width: `${(s.progress.done / s.progress.total) * 100}%`,
              }}
            />
          </span>
        )}
        <button
          className={s.reportOpen ? "on" : ""}
          onClick={() => s.setReport(!s.reportOpen)}
          title="Report: print readiness and per-node timing"
          aria-label={`Report${rep.errors > 0 ? ` ✖${rep.errors}` : ""}${rep.warnings > 0 ? ` ▲${rep.warnings}` : ""}`}
        >
          <Icon name="report" />
          {rep.errors > 0 && <span className="tb-badge err" aria-hidden="true">{rep.errors}</span>}
          {rep.warnings > 0 && <span className="tb-badge warn" aria-hidden="true">{rep.warnings}</span>}
        </button>
        <button
          className={s.perfOpen ? "on" : ""}
          onClick={() => s.setPerf(!s.perfOpen)}
          title="Perf: show engine workers and load"
          aria-label="Perf"
        >
          <Icon name="perf" />
        </button>
        <button
          className={s.consoleOpen ? "on" : ""}
          onClick={() => s.setConsole(!s.consoleOpen)}
          title="Console: the engine log"
          aria-label={`Console${s.logs.some((l) => l.level === "error") ? " ⚠" : ""}`}
        >
          <Icon name="console" />
          {s.logs.some((l) => l.level === "error") && <span className="tb-badge err" aria-hidden="true">!</span>}
        </button>
        <Status
          onGoto={(id) => {
            useStudio.getState().select(id);
            const n = rf.getNode(id);
            if (n) rf.setCenter(n.position.x + (n.measured?.width ?? 200) / 2, n.position.y + (n.measured?.height ?? 100) / 2, { zoom: Math.max(rf.getZoom(), 0.8), duration: 250 });
          }}
        />
      </header>
      {cmdOpen && (
        <CommandPalette actions={cmdActions} onClose={() => setCmdOpen(false)} />
      )}
      <main>
        <div className="canvas">
          {s.perfOpen && <Perf />}
          {s.reportOpen && <Report />}
          <ReactFlow
            nodes={s.nodes}
            edges={s.edges}
            nodeTypes={nodeTypes}
            edgeTypes={edgeTypes}
            onNodesChange={s.onNodesChange}
            onEdgesChange={s.onEdgesChange}
            onConnect={s.connect}
            isValidConnection={isValid}
            onNodeClick={(_, n) => s.select(n.id)}
            onPaneClick={() => s.select(null)}
            onNodeDrag={(_, n) => setSplitEdge(edgeToSplit(n.id)?.id ?? null)}
            onNodeDragStop={(_, n) => {
              const hit = edgeToSplit(n.id);
              setSplitEdge(null);
              if (hit) useStudio.getState().insertOnEdge(n.id, hit.id, hit.input, hit.output);
            }}
            onNodeDoubleClick={(_, n) => s.openBench(n.id)}
            deleteKeyCode={["Delete", "Backspace"]}
            colorMode="dark"
            fitView={false}
            minZoom={0.1}
            autoPanOnNodeDrag={false}
            translateExtent={extent}
            proOptions={{ hideAttribution: true }}
            defaultViewport={{ x: 20, y: 60, zoom: 0.7 }}
            defaultEdgeOptions={{
              style: { stroke: KIND_COLOR.any, strokeWidth: 2 },
            }}
          >
            <Background gap={24} color="#2a2f37" />
            <Controls showInteractive={false} />
            <MiniMap pannable zoomable />
          </ReactFlow>
          {!s.nodes.length && (
            <div className="hint">Drop a picture here (png, jpg, bmp, svg)</div>
          )}
          {drag && <div className="dropzone">Drop to add the image</div>}
        </div>
        <PanelResizer />
        <Inspector />
      </main>
      {s.consoleOpen && <Console height={170} />}
      <Workbench />
    </div>
  );
}

/** The names nodes had before the verb/noun rename, so the palette still finds them. */
const FORMER_NAMES: Record<string, string> = {
  tone_patterns: "Tone patterns",
  morphology: "Morphology",
  shapes_to_mask: "Shapes to mask",
  ring: "Margin expansion",
  pads: "Pads & mounting holes",
  hardware: "Screws, nuts & fit test",
};

export default function App() {
  return (
    <ReactFlowProvider>
      <Canvas />
    </ReactFlowProvider>
  );
}
