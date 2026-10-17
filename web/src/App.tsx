import { useCallback, useEffect, useRef, useState } from 'react'
import { Background, Controls, MiniMap, ReactFlow, ReactFlowProvider, ViewportPortal, useReactFlow, type Edge } from '@xyflow/react'
import { api, type NodeType, type Stage } from './api'
import Inspector from './Inspector'
import Console from './Console'
import Perf from './Perf'
import Workbench from './Workbench'
import StudioEdge from './StudioEdge'
import StudioNodeView, { KIND_COLOR } from './StudioNode'
import { etaSeconds, formatEta, useNow } from './eta'
import { laneSpan, redo, resizeLane, resetHistory, runGraph, signature, STORE_KEY, undo, useStudio, type StudioNode } from './store'

const nodeTypes = { studio: StudioNodeView }
const edgeTypes = { default: StudioEdge }
const LANES: { stage: Stage; title: string; edge: 'stencil' | 'post' | 'end' }[] = [
  { stage: 'trace', title: '1 · Trace — image to masks', edge: 'stencil' },
  { stage: 'stencil', title: '2 · Stencil — mask to 3D plate', edge: 'post' },
  { stage: 'post', title: '3 · Post-process — parts', edge: 'end' },
]

const DRAFT_DELAY = 150

/** The one-line state of the engine: working (with the time left when it is long), draft, error or ready. */
function Status() {
  const busy = useStudio((s) => s.busy)
  const error = useStudio((s) => s.error)
  const draft = useStudio((s) => s.draft)
  const runs = useStudio((s) => s.runs)
  const now = useNow(busy)
  const left = busy ? etaSeconds(runs, now) : null
  const text = error ? `⚠ ${error}` : busy ? `${draft ? 'refining' : 'working'}…${left !== null && left >= 3 ? ` ${formatEta(left)}` : ''}` : draft ? 'draft preview' : 'ready'
  return <span className={`status${busy ? ' busy' : ''}`} role="status" aria-live="polite">{text}</span>
}

function Canvas() {
  const s = useStudio()
  const rf = useReactFlow()
  const [drag, setDrag] = useState(false)
  const [pname, setPname] = useState('project')
  const [menu, setMenu] = useState<Stage | 'templates' | 'all' | null>(null)
  // wait for the new nodes to be measured, then bring the whole graph into view
  const fit = useCallback(() => { setTimeout(() => rf.fitView({ padding: 0.08, maxZoom: 1, duration: 250 }), 200) }, [rf])
  const bar = useRef<HTMLDivElement>(null)
  const tpl = useRef<HTMLDivElement>(null)
  const sliding = useRef(false)
  const hasSource = s.nodes.some((n) => n.data.type === 'source')
  useEffect(() => {
    const away = (e: MouseEvent) => { if (!bar.current?.contains(e.target as Node) && !tpl.current?.contains(e.target as Node)) setMenu(null) }
    const esc = (e: KeyboardEvent) => {
      if (e.key === 'Escape') return setMenu(null)
      const t = e.target as HTMLElement
      if (t.closest?.('input, textarea, select, [contenteditable]') || !(e.ctrlKey || e.metaKey)) return
      const k = e.key.toLowerCase()
      if (k === 'z' && !e.shiftKey) { e.preventDefault(); undo() }
      else if ((k === 'z' && e.shiftKey) || k === 'y') { e.preventDefault(); redo() }
      else if (k === 'c') useStudio.getState().copy()
      else if (k === 'v') { e.preventDefault(); useStudio.getState().paste() }
      else if (k === 'd') { e.preventDefault(); useStudio.getState().duplicate() }
    }
    const down = (e: PointerEvent) => { sliding.current = (e.target as HTMLElement | null)?.matches?.('input[type=range]') ?? false }
    const up = () => { if (sliding.current) { sliding.current = false; runGraph() } }
    window.addEventListener('mousedown', away); window.addEventListener('keydown', esc)
    window.addEventListener('pointerdown', down); window.addEventListener('pointerup', up); window.addEventListener('pointercancel', up)
    return () => {
      window.removeEventListener('mousedown', away); window.removeEventListener('keydown', esc)
      window.removeEventListener('pointerdown', down); window.removeEventListener('pointerup', up); window.removeEventListener('pointercancel', up)
    }
  }, [])

  useEffect(() => {
    Promise.all([api.nodeTypes(), api.restoreImages()]).then(([t]) => {
      s.setTypes(t)
      try {
        const saved = JSON.parse(localStorage.getItem(STORE_KEY) ?? 'null')
        if (saved) { s.load(saved.nodes, saved.edges); if (saved.nodes?.length) fit() }
        resetHistory()
      } catch { /* ignore a corrupt autosave */ }
    })
  }, [])

  // autosave and re-run only when something that affects the result changed
  const sig = signature(s)
  const first = useRef(true)
  useEffect(() => {
    if (!Object.keys(s.types).length) return
    try {
      localStorage.setItem(STORE_KEY, JSON.stringify({ nodes: s.nodes, edges: s.edges }))
    } catch {
      useStudio.setState({ error: 'autosave failed (browser storage is full or blocked): use Save project to keep your work' })
    }
    // one run per settled edit; it evaluates node by node, so the trace stage shows up first.
    // While a slider is held down a coarse draft runs instead, and the full run follows on release.
    const wait = first.current ? 0 : sliding.current ? DRAFT_DELAY : 350
    const full = setTimeout(() => runGraph(sliding.current), wait)
    first.current = false
    return () => clearTimeout(full)
  }, [sig, Object.keys(s.types).length])

  const addFile = useCallback(async (f: File, at?: { x: number; y: number }) => {
    try {
      const img = await api.upload(f)
      if (!useStudio.getState().nodes.length) { useStudio.getState().starter(img.id, img.name); fit() }
      else useStudio.getState().addNode('source', { image_id: img.id }, at, img.name)
    } catch (e) {
      useStudio.setState({ error: String(e instanceof Error ? e.message : e) })
    }
  }, [fit])

  const onDrop = (e: React.DragEvent) => {
    e.preventDefault()
    setDrag(false)
    const f = e.dataTransfer.files[0]
    if (f) addFile(f, rf.screenToFlowPosition({ x: e.clientX, y: e.clientY }))
  }

  const isValid = (c: { source: string; sourceHandle?: string | null; target: string; targetHandle?: string | null }) => {
    const nodes = useStudio.getState().nodes
    const a = nodes.find((n) => n.id === c.source), b = nodes.find((n) => n.id === c.target)
    if (!a || !b || a.id === b.id) return false
    const o = s.types[a.data.type]?.outputs.find((x) => x.name === c.sourceHandle)
    const i = s.types[b.data.type]?.inputs.find((x) => x.name === c.targetHandle)
    return !!o && !!i && (i.type === o.type || (i.type === 'any' && (o.type === 'image' || o.type === 'mask')))
  }

  const save = async () => {
    const p = await api.exportProject({ nodes: s.nodes, edges: s.edges })
    const url = URL.createObjectURL(new Blob([JSON.stringify(p)], { type: 'application/json' }))
    const a = document.createElement('a')
    a.href = url
    a.download = `${pname || 'project'}.stencil.json`
    a.hidden = true
    document.body.appendChild(a)
    a.click()
    a.remove()
    setTimeout(() => URL.revokeObjectURL(url), 10_000)
  }
  const open = async (f: File) => {
    try {
      const g = await api.importProject(JSON.parse(await f.text()))
      setPname(f.name.replace(/\.stencil\.json$|\.json$/, ''))
      s.load(g.nodes as StudioNode[], g.edges as Edge[])
      fit()
    } catch (e) {
      useStudio.setState({ error: `cannot open project: ${e instanceof Error ? e.message : e}` })
    }
  }

  const addFromMenu = (t: NodeType) => {
    const c = rf.screenToFlowPosition({ x: window.innerWidth * 0.3, y: 160 })
    const sp = laneSpan(s.lanes, t.stage)
    s.addNode(t.id, {}, { x: t.stage === 'trace' ? c.x : sp.x + 30, y: c.y })
    setMenu(null)
  }
  // fill each column to the height the page allows, then start the next one
  const menuStyle = (n: number) => {
    const rows = Math.max(4, Math.floor((window.innerHeight - 70) / 56))
    return { '--rows': Math.min(n, rows) } as React.CSSProperties
  }
  const popular = Object.values(s.types).sort((a, b) => (s.usage[b.id] ?? 0) - (s.usage[a.id] ?? 0) || a.label.localeCompare(b.label))
  const stageNodes = (st: Stage) => Object.values(s.types).filter((t) => t.stage === st)
  const lane = (l: (typeof LANES)[number]) => {
    const { x, w } = laneSpan(s.lanes, l.stage)
    // dragging a handle moves the boundary that follows this lane, so the next lane keeps its right edge
    const drag = (e: React.PointerEvent<HTMLDivElement>) => {
      e.preventDefault()
      e.stopPropagation()
      const el = e.currentTarget
      el.setPointerCapture(e.pointerId)
      const startX = e.clientX
      const zoom = rf.getViewport().zoom
      const base = s.lanes
      const at = l.edge === 'stencil' ? base.stencil : l.edge === 'post' ? base.post : base.end
      const move = (ev: PointerEvent) => useStudio.getState().setLanes(resizeLane(base, l.edge, at + (ev.clientX - startX) / zoom))
      const up = () => { el.removeEventListener('pointermove', move); el.removeEventListener('pointerup', up) }
      el.addEventListener('pointermove', move)
      el.addEventListener('pointerup', up)
    }
    return (
      <div key={l.stage} className={`lane lane-${l.stage}`} style={{ left: x, width: w }}>
        {l.title}
        <div className="lane-grip" title="Drag to resize this panel" onPointerDown={drag} />
      </div>
    )
  }

  return (
    <div className="app" onDragOver={(e) => { e.preventDefault(); setDrag(true) }} onDragLeave={() => setDrag(false)} onDrop={onDrop}>
      <header>
        <b>Stencil studio</b>
        <div className="palette" ref={bar}>
          {(['trace', 'stencil', 'post'] as Stage[]).map((st) => (
            <div key={st} className="pal">
              <button className={`stage-${st}${menu === st ? ' on' : ''}`} aria-haspopup="true" aria-expanded={menu === st} onClick={() => setMenu(menu === st ? null : st)}>+ {st}</button>
              {menu === st && <div className="menu" style={menuStyle(stageNodes(st).length)}>{stageNodes(st).map((t) => (
                <button key={t.id} title={t.doc} onClick={() => addFromMenu(t)}><b>{t.label}</b><small>{t.doc.split(/(?<=\.)\s/)[0]}</small></button>
              ))}</div>}
            </div>
          ))}
          <div className="pal">
            <button className={menu === 'all' ? 'on' : ''} aria-haspopup="true" aria-expanded={menu === 'all'} onClick={() => setMenu(menu === 'all' ? null : 'all')} title="Every node, most used first">+ All</button>
            {menu === 'all' && <div className="menu" style={menuStyle(popular.length)}>{popular.map((t) => {
              const n = s.usage[t.id] ?? 0
              return (
                <button key={t.id} className={`stage-${t.stage}`} title={`${t.doc}\n\n${n ? `Added ${n} time${n === 1 ? '' : 's'} in this browser` : 'Not used yet'}`} onClick={() => addFromMenu(t)}>
                  <b>{t.label}{n > 0 && <span className="cnt">{n}</span>}</b><small>{t.stage} · {t.doc.split(/(?<=\.)\s/)[0]}</small>
                </button>
              )
            })}</div>}
          </div>
        </div>
        <label className="btn">Open image…<input type="file" accept=".png,.jpg,.jpeg,.bmp,.svg" hidden onChange={(e) => e.target.files?.[0] && addFile(e.target.files[0])} /></label>
        <div className="pal" ref={tpl}>
          <button className={menu === 'templates' ? 'on' : ''} aria-haspopup="true" aria-expanded={menu === 'templates'} onClick={() => setMenu(menu === 'templates' ? null : 'templates')} title="Rebuild the graph around the current picture">Templates ▾</button>
          {menu === 'templates' && (
            <div className="menu wide">
              {([
                ['Simple', 'image → threshold → stencil → export. The quickest route to an STL.', s.starter],
                ['Colour tones', 'image → reduce colours → tone patterns → stencil. For pictures with shades.', s.colourStarter],
                ['Vector', 'image → threshold → trace → vector stencil. Smooth curves instead of pixels.', s.vectorStarter],
                ['Colour set', 'image → split colours → one stencil per colour → colour set. Layers with registration holes and pins, as one ZIP.', s.colourSetStarter],
              ] as const).map(([name, doc, make]) => (
                <button key={name} disabled={!hasSource} onClick={() => {
                  const src = s.nodes.find((n) => n.data.type === 'source')
                  if (src) { make(String(src.data.params.image_id), String(src.data.title ?? 'image')); fit() }
                  setMenu(null)
                }}><b>{name}</b><small>{doc}</small></button>
              ))}
              <small className="menu-note">{hasSource ? 'Replaces the current graph (Ctrl+Z undoes it).' : 'Open an image first.'}</small>
            </div>
          )}
        </div>
        <button title="Undo (Ctrl+Z)" disabled={!s.canUndo} onClick={undo}>↶ Undo</button>
        <button title="Redo (Ctrl+Shift+Z)" disabled={!s.canRedo} onClick={redo}>↷ Redo</button>
        <span className="spacer" />
        <label className="btn">Open project…<input type="file" accept=".json" hidden onChange={(e) => { const f = e.target.files?.[0]; e.target.value = ''; if (f) open(f) }} /></label>
        <input className="pname" value={pname} onChange={(e) => setPname(e.target.value)} aria-label="Project name" />
        <button onClick={save}>Save project</button>
        <button onClick={() => { if (s.nodes.length && !window.confirm('Discard the current graph? Save the project first to keep it.')) return; s.load([], []); localStorage.removeItem(STORE_KEY) }}>New</button>
        {s.busy && s.progress.total > 0 && <span className="gbar" role="progressbar" aria-label="Run progress" aria-valuemin={0} aria-valuemax={s.progress.total} aria-valuenow={Math.floor(s.progress.done)} title={`${Math.floor(s.progress.done)}/${s.progress.total} nodes`}><i style={{ width: `${(s.progress.done / s.progress.total) * 100}%` }} /></span>}
        <button className={s.perfOpen ? 'on' : ''} onClick={() => s.setPerf(!s.perfOpen)} title="Show engine workers and load">Perf</button>
        <button className={s.consoleOpen ? 'on' : ''} onClick={() => s.setConsole(!s.consoleOpen)}>Console{s.logs.some((l) => l.level === 'error') ? ' ⚠' : ''}</button>
        <Status />
      </header>
      <main>
        <div className="canvas">
          {s.perfOpen && <Perf />}
          <ReactFlow
            nodes={s.nodes} edges={s.edges} nodeTypes={nodeTypes} edgeTypes={edgeTypes}
            onNodesChange={s.onNodesChange} onEdgesChange={s.onEdgesChange} onConnect={s.connect}
            isValidConnection={isValid} onNodeClick={(_, n) => s.select(n.id)} onPaneClick={() => s.select(null)} onNodeDoubleClick={(_, n) => s.openBench(n.id)}
            deleteKeyCode={['Delete', 'Backspace']} colorMode="dark" fitView={false} minZoom={0.1} autoPanOnNodeDrag={false} translateExtent={[[-800, -600], [s.lanes.end + 800, 3600]]} proOptions={{ hideAttribution: true }}
            defaultViewport={{ x: 20, y: 60, zoom: 0.7 }} defaultEdgeOptions={{ style: { stroke: KIND_COLOR.any, strokeWidth: 2 } }}
          >
            <ViewportPortal><div className="lanes">{LANES.map(lane)}</div></ViewportPortal>
            <Background gap={24} color="#2a2f37" />
            <Controls showInteractive={false} />
            <MiniMap pannable zoomable />
          </ReactFlow>
          {!s.nodes.length && <div className="hint">Drop a picture here (png, jpg, bmp, svg)</div>}
          {drag && <div className="dropzone">Drop to add the image</div>}
        </div>
        <Inspector />
      </main>
      {s.consoleOpen && <Console height={170} />}
      <Workbench />
    </div>
  )
}

export default function App() {
  return <ReactFlowProvider><Canvas /></ReactFlowProvider>
}
