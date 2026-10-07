import { useCallback, useEffect, useRef, useState } from 'react'
import { Background, Controls, MiniMap, ReactFlow, ReactFlowProvider, ViewportPortal, useReactFlow, type Edge } from '@xyflow/react'
import { api, type Stage } from './api'
import Inspector from './Inspector'
import Console from './Console'
import Workbench from './Workbench'
import StudioEdge from './StudioEdge'
import StudioNodeView, { KIND_COLOR } from './StudioNode'
import { LANE_X, runGraph, signature, useStudio, type StudioNode } from './store'

const nodeTypes = { studio: StudioNodeView }
const edgeTypes = { default: StudioEdge }
const LANES: { stage: Stage; title: string; x: number; w: number }[] = [
  { stage: 'trace', title: '1 · Trace — image to masks', x: 0, w: 1150 },
  { stage: 'stencil', title: '2 · Stencil — mask to 3D plate', x: LANE_X.stencil, w: 530 },
  { stage: 'post', title: '3 · Post-process — parts', x: LANE_X.post, w: 400 },
]
const STORE_KEY = 'studio.graph.v1'

function Canvas() {
  const s = useStudio()
  const rf = useReactFlow()
  const [drag, setDrag] = useState(false)
  const [names, setNames] = useState<string[]>([])
  const [pname, setPname] = useState('project')
  const [menu, setMenu] = useState<Stage | null>(null)
  const bar = useRef<HTMLDivElement>(null)
  useEffect(() => {
    const away = (e: MouseEvent) => { if (!bar.current?.contains(e.target as Node)) setMenu(null) }
    const esc = (e: KeyboardEvent) => e.key === 'Escape' && setMenu(null)
    window.addEventListener('mousedown', away); window.addEventListener('keydown', esc)
    return () => { window.removeEventListener('mousedown', away); window.removeEventListener('keydown', esc) }
  }, [])

  useEffect(() => {
    api.nodeTypes().then((t) => {
      s.setTypes(t)
      try {
        const saved = JSON.parse(localStorage.getItem(STORE_KEY) ?? 'null')
        if (saved) s.load(saved.nodes, saved.edges)
      } catch { /* ignore a corrupt autosave */ }
    })
    api.projects().then(setNames).catch(() => {})
  }, [])

  // autosave and re-run only when something that affects the result changed
  const sig = signature(s)
  const first = useRef(true)
  useEffect(() => {
    if (!Object.keys(s.types).length) return
    localStorage.setItem(STORE_KEY, JSON.stringify({ nodes: s.nodes, edges: s.edges }))
    // trace nodes answer almost live; the heavier stages follow once the edits settle
    const quick = first.current ? undefined : setTimeout(() => runGraph('trace'), 100)
    const full = setTimeout(() => runGraph('all'), first.current ? 0 : 700)
    first.current = false
    return () => { clearTimeout(quick); clearTimeout(full) }
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
    await api.saveProject(pname, { nodes: s.nodes, edges: s.edges })
    setNames(await api.projects())
  }
  const open = async (n: string) => {
    if (!n) return
    const p = await api.loadProject(n)
    setPname(n)
    s.load(p.nodes as StudioNode[], p.edges as Edge[])
  }

  const stageNodes = (st: Stage) => Object.values(s.types).filter((t) => t.stage === st)
  const lane = (l: (typeof LANES)[number]) => (
    <div key={l.stage} className={`lane lane-${l.stage}`} style={{ left: l.x, width: l.w }}>{l.title}</div>
  )

  return (
    <div className="app" onDragOver={(e) => { e.preventDefault(); setDrag(true) }} onDragLeave={() => setDrag(false)} onDrop={onDrop}>
      <header>
        <b>Stencil studio</b>
        <div className="palette" ref={bar}>
          {(['trace', 'stencil', 'post'] as Stage[]).map((st) => (
            <div key={st} className="pal">
              <button className={`stage-${st}${menu === st ? ' on' : ''}`} onClick={() => setMenu(menu === st ? null : st)}>+ {st}</button>
              {menu === st && <div className="menu">{stageNodes(st).map((t) => (
                <button key={t.id} title={t.doc} onClick={() => { const c = rf.screenToFlowPosition({ x: window.innerWidth * 0.3, y: 160 }); s.addNode(t.id, {}, { x: st === 'trace' ? c.x : LANE_X[st] + 30, y: c.y }); setMenu(null) }}>{t.label}</button>
              ))}</div>}
            </div>
          ))}
        </div>
        <label className="btn">Open image…<input type="file" accept=".png,.jpg,.jpeg,.bmp,.svg" hidden onChange={(e) => e.target.files?.[0] && addFile(e.target.files[0])} /></label>
        <button title="Rebuild the graph as image → reduce colours → tone patterns → stencil, from the current image" onClick={() => {
          const src = s.nodes.find((n) => n.data.type === 'source')
          if (src) s.colourStarter(String(src.data.params.image_id), String(src.data.title ?? 'image'))
        }}>Colour template</button>
        <span className="spacer" />
        <select value="" onChange={(e) => open(e.target.value)}><option value="">Open project…</option>{names.map((n) => <option key={n}>{n}</option>)}</select>
        <input className="pname" value={pname} onChange={(e) => setPname(e.target.value)} aria-label="Project name" />
        <button onClick={save}>Save</button>
        <button onClick={() => { s.load([], []); localStorage.removeItem(STORE_KEY) }}>New</button>
        {s.busy && s.progress.total > 0 && <span className="gbar" title={`${s.progress.done}/${s.progress.total} nodes`}><i style={{ width: `${(s.progress.done / s.progress.total) * 100}%` }} /></span>}
        <button className={s.consoleOpen ? 'on' : ''} onClick={() => s.setConsole(!s.consoleOpen)}>Console{s.logs.some((l) => l.level === 'error') ? ' ⚠' : ''}</button>
        <span className={`status${s.busy ? ' busy' : ''}`}>{s.error ? `⚠ ${s.error}` : s.busy ? 'working…' : 'ready'}</span>
      </header>
      <main>
        <div className="canvas">
          <ReactFlow
            nodes={s.nodes} edges={s.edges} nodeTypes={nodeTypes} edgeTypes={edgeTypes}
            onNodesChange={s.onNodesChange} onEdgesChange={s.onEdgesChange} onConnect={s.connect}
            isValidConnection={isValid} onNodeClick={(_, n) => s.select(n.id)} onPaneClick={() => s.select(null)} onNodeDoubleClick={(_, n) => s.openBench(n.id)}
            deleteKeyCode={['Delete', 'Backspace']} colorMode="dark" fitView={false} minZoom={0.1} proOptions={{ hideAttribution: true }}
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
