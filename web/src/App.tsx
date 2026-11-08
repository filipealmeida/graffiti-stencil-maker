import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useShallow } from 'zustand/react/shallow'
import {
  Background,
  Controls,
  MiniMap,
  ReactFlow,
  ReactFlowProvider,
  useReactFlow,
} from "@xyflow/react";
import { api, type Kind, type NodeType, type Stage } from "./api";
import Icon from "./Icon";
import PanelResizer from './PanelResizer'
import Inspector from "./Inspector";
import Console from "./Console";
import Perf from "./Perf";
import Report, { useReport } from "./Report";
import History from "./History";
import Toasts from "./Toasts";
import { randomName } from "./names";
import { forgetFile, saveProject, exportEverything, recentProjects, safeName } from "./project";
import Workbench from "./Workbench";
import ActionLog from './ActionLog'
import StudioEdge from "./StudioEdge";
import StudioNodeView, { KIND_COLOR } from "./StudioNode";
import { STAGE_LABEL, canLink } from "./ports";
import Status from "./Status";
import CommandPalette from "./CommandPalette";
import {
  stageColumn,
  WORLD_END,
  redo,
  resetHistory,
  runGraph,
  rerun,
  stopRunning,
  signature,
  checkpoint,
  STORE_KEY,
  undo,
  useStudio,
  logAction,
  reportError,
} from "./store";
import useStudioKeyboard from './useStudioKeyboard'
import useGraphLifecycle from './useGraphLifecycle'
import useProjectAutosave, { readAutosave } from './useProjectAutosave'
import Tooltip from './Tooltip'
import messages from './messages'
import useProjectIO from './useProjectIO'
import CatalogueLabel from './CatalogueLabel'

const nodeTypes = { studio: StudioNodeView };
const edgeTypes = { default: StudioEdge };
function Canvas() {
  const s = useStudio(useShallow((state) => ({
    nodes: state.nodes,
    edges: state.edges,
    types: state.types,
    pname: state.pname,
    setPname: state.setPname,
    savedSig: state.savedSig,
    savedAt: state.savedAt,
    addLogs: state.addLogs,
    setTypes: state.setTypes,
    load: state.load,
    markSaved: state.markSaved,
    toast: state.toast,
    usage: state.usage,
    starter: state.starter,
    colourStarter: state.colourStarter,
    vectorStarter: state.vectorStarter,
    collageStarter: state.collageStarter,
    colourSetStarter: state.colourSetStarter,
    tiledStarter: state.tiledStarter,
    tilesFirstStarter: state.tilesFirstStarter,
    cutSheetStarter: state.cutSheetStarter,
    addNode: state.addNode,
    canUndo: state.canUndo,
    canRedo: state.canRedo,
    busy: state.busy,
    reportOpen: state.reportOpen,
    setReport: state.setReport,
    perfOpen: state.perfOpen,
    setPerf: state.setPerf,
    consoleOpen: state.consoleOpen,
    setConsole: state.setConsole,
    hasLogError: state.logs.some((line) => line.level === "error"),
    onNodesChange: state.onNodesChange,
    onEdgesChange: state.onEdgesChange,
    connect: state.connect,
    select: state.select,
    openBench: state.openBench,
    progress: state.progress,
  })));
  const rep = useReport();
  const rf = useReactFlow();
  const [drag, setDrag] = useState(false);
  const [draftReady, setDraftReady] = useState(false)
  const [draftProblem, setDraftProblem] = useState<string | null>(null)
  const pname = s.pname;
  const setPname = s.setPname;
  const [pmenu, setPmenu] = useState(false);
  const dirty = s.nodes.length > 0 && s.savedSig !== signature(s);
  const [menu, setMenu] = useState<Stage | "templates" | "all" | null>(null);
  const [cmdOpen, setCmdOpen] = useState(false);
  // Wait for measurement, then centre the graph without making its labels too small.
  const fit = useCallback(() => {
    setTimeout(
      () => rf.fitView({ padding: 0.08, minZoom: 0.8, maxZoom: 1, duration: 250 }),
      200,
    );
  }, [rf]);
  const { openProject: open, addFile } = useProjectIO(fit)
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
  const pm = useRef<HTMLDivElement>(null);
  const sliding = useRef(false);
  const hasSource = s.nodes.some((n) => n.data.type === "source");
  const closeMenus = useCallback(() => {
    setMenu(null)
    setPmenu(false)
    setCmdOpen(false)
  }, [])
  const toggleCommandPalette = useCallback(() => {
    setMenu(null)
    setPmenu(false)
    setCmdOpen((open) => !open)
  }, [])
  useStudioKeyboard(closeMenus, toggleCommandPalette)
  useEffect(() => {
    const away = (e: MouseEvent) => {
      if (
        !bar.current?.contains(e.target as Node) &&
        !tpl.current?.contains(e.target as Node)
      )
        setMenu(null);
      if (!pm.current?.contains(e.target as Node)) setPmenu(false);
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
    window.addEventListener("mousedown", away, true);
    window.addEventListener("pointerdown", down);
    window.addEventListener("pointerup", up);
    window.addEventListener("pointercancel", up);
    return () => {
      window.removeEventListener("mousedown", away, true);
      window.removeEventListener("pointerdown", down);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("pointercancel", up);
    };
  }, []);

  // the first console line says exactly which build is running
  useEffect(() => {
    if (announced) return;
    announced = true;
    api.threads().then((e) => {
      const b = __BUILD__;
      const engine = e.version === b.version ? `engine ${e.version}` : `engine ${e.version} (page ${b.version}: stale cache?)`;
      const threads = e.n > 1 ? `${e.n} threads` : `single thread (${e.why})`;
      const msg = `Graffiti stencil studio v${b.version} · build ${b.build} (${b.commit}) · built ${b.time} · ${engine} · ${threads}`;
      console.info(msg);
      s.addLogs([{ ts: new Date().toISOString().slice(0, 23), ms: null, t: Date.now(), level: "info", node: "", type: "studio", msg }]);
    }).catch(() => {});
  }, []);

  useEffect(() => {
    Promise.all([api.nodeTypes(), api.restoreImages()]).then(([t]) => {
      s.setTypes(t);
      try {
        const saved = readAutosave()
        if (saved) {
          s.load(saved.nodes, saved.edges);
          if (saved.nodes?.length) fit();
        }
        resetHistory();
      } catch (e) {
        const message = `Could not restore the browser draft: ${e instanceof Error ? e.message : String(e)}`
        setDraftProblem(message)
        reportError(message)
      } finally {
        setDraftReady(true)
      }
    }).catch((e) => {
      reportError(e)
      setDraftReady(true)
    })
  }, []);

  const sig = signature(s);
  const autosave = useProjectAutosave(draftReady && !draftProblem)
  useGraphLifecycle(sig, draftReady, sliding)

  const onDrop = (e: React.DragEvent) => {
    e.preventDefault();
    setDrag(false);
    const f = e.dataTransfer.files[0];
    if (!f) return;
    if (/\.json$/i.test(f.name)) void open(f);
    else addFile(f, rf.screenToFlowPosition({ x: e.clientX, y: e.clientY }));
  };

  /** Can an output of this kind feed an input of that kind? The same rule as drawing a connection by hand. */
  const kindFits = (out: string, inp: string) => canLink(out as Kind, inp as Kind);

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
    return !!o && !!i && canLink(o.type, i.type);
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
      "Collage",
      "image placed twice on one canvas (Place moves and scales the second) → threshold → stencil → bridges. Replace the copies with your own pictures, text or outlines.",
      s.collageStarter,
    ],
    [
      "Multi-colour pack",
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
      "image → threshold → tile plan → stencil (per tile) → pads & holes → screws. Cut into tiles before bridging, so every tile is island-free by itself.",
      s.tilesFirstStarter,
    ],
    [
      "Cut sheet",
      "image → threshold → trace → tile plan → cut sheet. Flat parts for a laser or CNC router, bridged and laid out on sheets as SVG and DXF.",
      s.cutSheetStarter,
    ],
  ] as const;
  const runTemplate = (make: (imageId: string, title: string) => void) => {
    const src = s.nodes.find((n) => n.data.type === "source");
    if (src) {
      checkpoint()
      make(String(src.data.params.image_id), String(src.data.title ?? "image"));
      checkpoint()
      s.toast(messages.actions.appliedTemplate, { undo })
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
      { id: "stop", title: "Stop all processing", note: "Cancels the running evaluation at once; Rerun resumes", run: () => stopRunning() },
      ...current.map((t) => ({
        id: `node:${t.id}`,
        title: t.label,
        experimental: t.experimental,
        note: `${STAGE_LABEL[t.stage]} · ${t.doc.split(/(?<=\.)\s/)[0]}${FORMER_NAMES[t.id] ? ` · was ${FORMER_NAMES[t.id]}` : ""}`,
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
      <ActionLog />
      <header>
        <h1>Stencil studio</h1>
        <div className="palette" ref={bar}>
          {(["trace", "stencil", "post"] as Stage[]).map((st) => (
            <div key={st} className="pal">
              <button
                className={`stage-${st}${menu === st ? " on" : ""}`}
                aria-haspopup="true"
                aria-expanded={menu === st}
                onClick={() => { setPmenu(false); setMenu(menu === st ? null : st) }}
              >
                + {STAGE_LABEL[st]}
              </button>
              {menu === st && (
                <div className="menu" style={menuStyle(stageNodes(st).length)}>
                  {stageNodes(st).map((t) => (
                    <button
                      key={t.id}
                      title={t.doc}
                      onClick={() => addFromMenu(t)}
                    >
                      <b><CatalogueLabel type={t} /></b>
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
              onClick={() => { setPmenu(false); setMenu(menu === "all" ? null : "all") }}
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
                        <CatalogueLabel type={t} />
                        {n > 0 && <span className="cnt">{n}</span>}
                      </b>
                      <small>
                        {STAGE_LABEL[t.stage]} · {t.doc.split(/(?<=\.)\s/)[0]}
                      </small>
                    </button>
                  );
                })}
              </div>
            )}
          </div>
        </div>
        <span className="sep" />
        <label className="btn icon-btn" title="Open image…">
          <Icon name="image" />
          <span>Open image</span>
          <input
            type="file"
            accept=".png,.jpg,.jpeg,.bmp,.svg"
            hidden
            onChange={(e) => e.target.files?.[0] && addFile(e.target.files[0])}
          />
        </label>
        <div className="pal" ref={tpl}>
        <Tooltip content="Templates: rebuild the graph around the current picture"><button
          className={menu === "templates" ? "on" : ""}
          aria-haspopup="true"
          aria-expanded={menu === "templates"}
          onClick={() => { setPmenu(false); setMenu(menu === "templates" ? null : "templates") }}
          aria-label="Templates"
        >
          <Icon name="templates" />
          <span>Templates</span>
        </button></Tooltip>
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
        <Tooltip content={messages.toolbar.commandPalette}><button
          onClick={() => { setMenu(null); setPmenu(false); setCmdOpen(true) }}
          aria-label="Command palette"
        >
          <Icon name="command" />
        </button></Tooltip>
        <span className="sep" />
        <History />
        <Tooltip content={messages.toolbar.undo}><button aria-label="Undo" disabled={!s.canUndo} onClick={undo}>
          <Icon name="undo" />
        </button></Tooltip>
        <Tooltip content={messages.toolbar.redo}><button
          aria-label="Redo"
          disabled={!s.canRedo}
          onClick={redo}
        >
          <Icon name="redo" />
        </button></Tooltip>
        <span className="spacer" />
        <div className="pal project" ref={pm}>
          <input
            className="pname"
            value={pname}
            onChange={(e) => setPname(e.target.value)}
            aria-label="Project name"
          />
          {dirty && <span className="dirty" title={messages.project.unsaved} role="img" aria-label="Unsaved changes">●</span>}
          {!dirty && s.savedAt && <span className="mut saved">Saved {s.savedAt}</span>}
          <Tooltip content={messages.toolbar.project}><button
            className={pmenu ? "on" : ""}
            aria-haspopup="true"
            aria-expanded={pmenu}
            onClick={() => { setMenu(null); setPmenu(!pmenu) }}
          >
            <Icon name="folder" /> Project ▾
          </button></Tooltip>
          {pmenu && (
            <div className="menu pmenu" role="menu" onClick={() => setPmenu(false)}>
              <button
                role="menuitem"
                onClick={() => {
                  const old = pname;
                  checkpoint();
                  s.load([], []);
                  checkpoint();
                  forgetFile();
                  setPname(randomName());
                  localStorage.removeItem(STORE_KEY);
                  setDraftProblem(null);
                  s.toast(messages.project.started, { undo: () => { undo(); setPname(old); } });
                }}
              ><b>New</b><small>Start empty (undo brings the graph back)</small></button>
              <button role="menuitem" onClick={() => document.getElementById("open-project")?.click()}><b>Open…</b><small>Ctrl+O, or drop a .stencil.json on the canvas</small></button>
              <button role="menuitem" onClick={() => void saveProject().catch((e) => reportError(e))} title="Save the project file (Ctrl+S)"><b>Save</b><small>Overwrites {safeName(pname)}.stencil.json · Ctrl+S</small></button>
              <button role="menuitem" onClick={() => void saveProject(true).catch((e) => reportError(e))}><b>Save as…</b><small>Pick a new file name or place</small></button>
              <button role="menuitem" onClick={() => void exportEverything().catch((e) => reportError(e))}><b>Export everything…</b><small>One zip: project, models, findings</small></button>
              {recentProjects().length > 0 && <small className="menu-note">Recently saved: {recentProjects().join(", ")}</small>}
            </div>
          )}
          <input
            id="open-project"
            type="file"
            accept=".json"
            hidden
            aria-label="Open project file"
            onChange={(e) => {
              const f = e.target.files?.[0];
              e.target.value = "";
              if (f) open(f);
            }}
          />
        </div>
        <span className="sep" />
        {s.busy ? (
          <Tooltip content="Stop all processing now (Ctrl+.). Finished nodes keep their results; Rerun picks up again."><button
            onClick={stopRunning}
            aria-label="Stop processing"
          >
            <Icon name="stop" />
          </button></Tooltip>
        ) : (
          <Tooltip content="Rerun the whole graph (Ctrl+Enter). Unchanged nodes come from the cache."><button
            onClick={rerun}
            aria-label="Rerun"
          >
            <Icon name="rerun" />
          </button></Tooltip>
        )}
        <Tooltip content={messages.toolbar.report}><button
          className={s.reportOpen ? "on" : ""}
          onClick={() => s.setReport(!s.reportOpen)}
          aria-label={`Report${rep.errors > 0 ? ` ✖${rep.errors}` : ""}${rep.warnings > 0 ? ` ▲${rep.warnings}` : ""}`}
        >
          <Icon name="report" />
          {rep.errors > 0 && <span className="tb-badge err" aria-hidden="true">{rep.errors}</span>}
          {rep.warnings > 0 && <span className="tb-badge warn" aria-hidden="true">{rep.warnings}</span>}
        </button></Tooltip>
        <Tooltip content={messages.toolbar.performance}><button
          className={s.perfOpen ? "on" : ""}
          onClick={() => s.setPerf(!s.perfOpen)}
          aria-label="Perf"
        >
          <Icon name="perf" />
        </button></Tooltip>
        <Tooltip content={messages.toolbar.console}><button
          className={s.consoleOpen ? "on" : ""}
          onClick={() => s.setConsole(!s.consoleOpen)}
          aria-label={`Console${s.hasLogError ? " ⚠" : ""}`}
        >
          <Icon name="console" />
          {s.hasLogError && <span className="tb-badge err" aria-hidden="true">!</span>}
        </button></Tooltip>
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
            onConnectStart={(_, p) => p.nodeId && p.handleId && useStudio.setState({ dragFrom: { node: p.nodeId, port: p.handleId, dir: p.handleType === 'source' ? 'out' : 'in' } })}
            onConnectEnd={(_, state) => {
              if (!state.isValid) logAction('Connection not applied: no compatible input selected', state.fromNode?.id ?? '');
              useStudio.setState({ dragFrom: null });
            }}
            onMoveEnd={(_, viewport) => logAction(`Canvas view: zoom ${viewport.zoom.toFixed(2)}, position ${viewport.x.toFixed(0)}, ${viewport.y.toFixed(0)}`)}
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
            defaultViewport={{ x: 20, y: 60, zoom: 0.7 }}
            defaultEdgeOptions={{
              style: { stroke: KIND_COLOR.any, strokeWidth: 2 },
            }}
          >
            <Background gap={24} color="#2a2f37" />
            <Controls showInteractive={false} />
            {s.nodes.length > 0 && <MiniMap pannable zoomable />}
          </ReactFlow>
          {!s.nodes.length && (
            <div className="hint">
              <div className="empty-card">
                <h2>Make a stencil</h2>
                <p>Drop a picture (png, jpg, bmp, svg) or a 3D model (stl, obj) here.</p>
                <label className="btn primary">
                  Open image…
                  <input type="file" accept=".png,.jpg,.jpeg,.bmp,.svg" hidden aria-label="Choose an image" onChange={(e) => e.target.files?.[0] && addFile(e.target.files[0])} />
                </label>
                <button onClick={() => document.getElementById("open-project")?.click()}>Open a project…</button>
                <p className="mut">Start from a template after the image is in: tiles, colours, vector.</p>
              </div>
            </div>
          )}
          {drag && <div className="dropzone">Drop to add the image</div>}
        </div>
        <PanelResizer />
        <Inspector />
      </main>
      <Console height={170} />
      <Toasts />
      <footer className="statusbar">
        <Status
          onGoto={(id) => {
            useStudio.getState().select(id);
            const n = rf.getNode(id);
            if (n) rf.setCenter(n.position.x + (n.measured?.width ?? 200) / 2, n.position.y + (n.measured?.height ?? 100) / 2, { zoom: Math.max(rf.getZoom(), 0.8), duration: 250 });
          }}
        />
        {autosave.state === 'saving' && <span className="draft-status mut" role="status">{messages.project.savingDraft}</span>}
        {autosave.state === 'saved' && autosave.time && <span className="draft-status mut" role="status" title={messages.project.draftDescription}>{messages.project.draftSaved} {autosave.time}</span>}
        {draftProblem && <span className="draft-status draft-problem" role="status" title={draftProblem}>
          <span>{draftProblem}</span>
          <button className="text-button" onClick={() => {
            if (!window.confirm(messages.project.discardDraftConfirm)) return
            try {
              localStorage.removeItem(STORE_KEY)
              setDraftProblem(null)
            } catch (error) {
              reportError(error)
            }
          }}>{messages.project.discardDraft}</button>
        </span>}
        {autosave.state === 'error' && <span className="draft-status err" role="status" title={autosave.error}>{messages.project.autosaveFailed}</span>}
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
      </footer>
      <Workbench />
    </div>
  );
}

/** The names nodes had before the verb/noun rename, so the palette still finds them. */
const FORMER_NAMES: Record<string, string> = {
  tone_patterns: "Tone patterns",
  morphology: "Morphology",
  shapes_to_mask: "Shapes to mask",
  pads: "Pads & mounting holes",
  hardware: "Screws, nuts & fit test",
  tile_layout: "Tile layout",
  remesh: "Remesh",
  ring: "Margin expansion · Expand margin",
  colour_set: "Colour set",
  remove_islands: "Remove islands",
};

let announced = false;

export default function App() {
  return (
    <ReactFlowProvider>
      <Canvas />
    </ReactFlowProvider>
  );
}
