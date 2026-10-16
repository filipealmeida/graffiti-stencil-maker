import { useCallback, useEffect, useRef, useState } from 'react'
import { Background, Controls, MiniMap, ReactFlow, ReactFlowProvider, ViewportPortal, useReactFlow, type Edge } from '@xyflow/react'
import { api, type Stage } from './api'
import Inspector from './Inspector'
import Console from './Console'
import Workbench from './Workbench'
import StudioEdge from './StudioEdge'
import StudioNodeView, { KIND_COLOR } from './StudioNode'
import { laneSpan, redo, resizeLane, resetHistory, runGraph, signature, undo, useStudio, type StudioNode } from './store'

const nodeTypes = { studio: StudioNodeView }
const edgeTypes = { default: StudioEdge }
const LANES: { stage: Stage; title: string; edge: 'stencil' | 'post' | 'end' }[] = [
  { stage: 'trace', title: '1 · Trace — image to masks', edge: 'stencil' },
  { stage: 'stencil', title: '2 · Stencil — mask to 3D plate', edge: 'post' },
  { stage: 'post', title: '3 · Post-process — parts', edge: 'end' },
]
const STORE_KEY = 'studio.graph.v1'

function Canvas() {
  const s = useStudio()
  const rf = useReactFlow()
  const [drag, setDrag] = useState(false)
  const [pname, setPname] = useState('project')
  const [menu, setMenu] = useState<Stage | null>(null)
  const bar = useRef<HTMLDivElement>(null)
  useEffect(() => {
    const away = (e: MouseEvent) => { if (!bar.current?.contains(e.target as Node)) setMenu(null) }
    const esc = (e: KeyboardEvent) => {
      if (e.key === 'Escape') return setMenu(null)
      const t = e.target as HTMLElement
      if (t.closest?.('input, textarea, select, [contenteditable]') || !(e.ctrlKey || e.metaKey)) return
      const k = e.key.toLowerCase()
      if (k === 'z' && !e.shiftKey) { e.preventDefault(); undo() }
      else if ((k === 'z' && e.shiftKey) || k === 'y') { e.preventDefault(); redo() }
    }
    window.addEventListener('mousedown', away); window.addEventListener('keydown', esc)
    return () => { window.removeEventListener('mousedown', away); window.removeEventListener('keydown', esc) }
  }, [])

  useEffect(() => {
    Promise.all([api.nodeTypes(), api.restoreImages()]).then(([t]) => {
      s.setTypes(t)
      try {
        const saved = JSON.parse(localStorage.getItem(STORE_KEY) ?? 'null')
        if (saved) s.load(saved.nodes, saved.edges)
        resetHistory()
      } catch { /* ignore a corrupt autosave */ }
    })
  }, [])

  // autosave and re-run only when something that affects the result changed
  const sig = signature(s)
  const first = useRef(true)
  useEffect(() => {
    if (!Object.keys(s.types).length) return
    localStorage.setItem(STORE_KEY, JSON.stringify({ nodes: s.nodes, edges: s.edges }))
    // one run per settled edit; it evaluates node by node, so the trace stage shows up first
    const full = setTimeout(runGraph, first.current ? 0 : 350)
    first.current = false
    return () => clearTimeout(full)
  }, [sig, Object.keys(s.types).length])

  const addFile = useCallback(async (f: File, at?: { x: number; y: number }) => {
    try {
      const img = await api.upload(f)
      if (!useStudio.getState().nodes.length) useStudio.getState().starter(img.id, img.name)
      else useStudio.getState().addNode('source', { image_id: img.id }, at, img.name)
    } catch (e) {
      useStudio.setState({ error: String(e instanceof Error ? e.message : e) })
    }
  }, [])

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
    } catch (e) {
      useStudio.setState({ error: `cannot open project: ${e instanceof Error ? e.message : e}` })
    }
  }

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
              <button className={`stage-${st}${menu === st ? ' on' : ''}`} onClick={() => setMenu(menu === st ? null : st)}>+ {st}</button>
              {menu === st && <div className="menu">{stageNodes(st).map((t) => (
                <button key={t.id} title={t.doc} onClick={() => { const c = rf.screenToFlowPosition({ x: window.innerWidth * 0.3, y: 160 }); const sp = laneSpan(s.lanes, st); s.addNode(t.id, {}, { x: st === 'trace' ? c.x : sp.x + 30, y: c.y }); setMenu(null) }}>{t.label}</button>
              ))}</div>}
            </div>
          ))}
        </div>
        <label className="btn">Open image…<input type="file" accept=".png,.jpg,.jpeg,.bmp,.svg" hidden onChange={(e) => e.target.files?.[0] && addFile(e.target.files[0])} /></label>
        <button title="Rebuild the graph as image → reduce colours → tone patterns → stencil, from the current image" onClick={() => {
          const src = s.nodes.find((n) => n.data.type === 'source')
          if (src) s.colourStarter(String(src.data.params.image_id), String(src.data.title ?? 'image'))
        }}>Colour template</button>
        <button title="Rebuild the graph as image → threshold → trace → vector stencil, from the current image" onClick={() => {
          const src = s.nodes.find((n) => n.data.type === 'source')
          if (src) s.vectorStarter(String(src.data.params.image_id), String(src.data.title ?? 'image'))
        }}>Vector template</button>
        <button title="Undo (Ctrl+Z)" disabled={!s.canUndo} onClick={undo}>↶ Undo</button>
        <button title="Redo (Ctrl+Shift+Z)" disabled={!s.canRedo} onClick={redo}>↷ Redo</button>
        <span className="spacer" />
        <label className="btn">Open project…<input type="file" accept=".json" hidden onChange={(e) => { const f = e.target.files?.[0]; e.target.value = ''; if (f) open(f) }} /></label>
        <input className="pname" value={pname} onChange={(e) => setPname(e.target.value)} aria-label="Project name" />
        <button onClick={save}>Save project</button>
        <button onClick={() => { if (s.nodes.length && !window.confirm('Discard the current graph? Save the project first to keep it.')) return; s.load([], []); localStorage.removeItem(STORE_KEY) }}>New</button>
        {s.busy && s.progress.total > 0 && <span className="gbar" title={`${Math.floor(s.progress.done)}/${s.progress.total} nodes`}><i style={{ width: `${(s.progress.done / s.progress.total) * 100}%` }} /></span>}
        <button className={s.consoleOpen ? 'on' : ''} onClick={() => s.setConsole(!s.consoleOpen)}>Console{s.logs.some((l) => l.level === 'error') ? ' ⚠' : ''}</button>
        <span className={`status${s.busy ? ' busy' : ''}`}>{s.error ? `⚠ ${s.error}` : s.busy ? 'working…' : 'ready'}</span>
      </header>
      <main>
        <div className="canvas">
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
