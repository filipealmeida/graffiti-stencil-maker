import { useCallback, useEffect, useRef, useState } from 'react'
import { Background, Controls, MiniMap, ReactFlow, ReactFlowProvider, ViewportPortal, useReactFlow, type Edge } from '@xyflow/react'
import { api, type Stage } from './api'
import Inspector from './Inspector'
import StudioNodeView, { KIND_COLOR } from './StudioNode'
import { LANE_X, runGraph, signature, useStudio, type StudioNode } from './store'

const nodeTypes = { studio: StudioNodeView }
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
    const t = setTimeout(runGraph, first.current ? 0 : 500)
    first.current = false
    return () => clearTimeout(t)
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
        <div className="palette">
          {(['trace', 'stencil', 'post'] as Stage[]).map((st) => (
            <details key={st}>
              <summary className={`stage-${st}`}>+ {st}</summary>
              <div className="menu">{stageNodes(st).map((t) => (
                <button key={t.id} title={t.doc} onClick={() => { const c = rf.screenToFlowPosition({ x: window.innerWidth * 0.3, y: 160 }); s.addNode(t.id, {}, { x: st === 'trace' ? c.x : LANE_X[st] + 30, y: c.y }) }}>{t.label}</button>
              ))}</div>
            </details>
          ))}
        </div>
        <label className="btn">Open image…<input type="file" accept=".png,.jpg,.jpeg,.bmp,.svg" hidden onChange={(e) => e.target.files?.[0] && addFile(e.target.files[0])} /></label>
        <span className="spacer" />
        <select value="" onChange={(e) => open(e.target.value)}><option value="">Open project…</option>{names.map((n) => <option key={n}>{n}</option>)}</select>
        <input className="pname" value={pname} onChange={(e) => setPname(e.target.value)} aria-label="Project name" />
        <button onClick={save}>Save</button>
        <button onClick={() => { s.load([], []); localStorage.removeItem(STORE_KEY) }}>New</button>
        <span className={`status${s.busy ? ' busy' : ''}`}>{s.error ? `⚠ ${s.error}` : s.busy ? 'working…' : 'ready'}</span>
      </header>
      <main>
        <div className="canvas">
          <ReactFlow
            nodes={s.nodes} edges={s.edges} nodeTypes={nodeTypes}
            onNodesChange={s.onNodesChange} onEdgesChange={s.onEdgesChange} onConnect={s.connect}
            isValidConnection={isValid} onNodeClick={(_, n) => s.select(n.id)} onPaneClick={() => s.select(null)}
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
    </div>
  )
}

export default function App() {
  return <ReactFlowProvider><Canvas /></ReactFlowProvider>
}
