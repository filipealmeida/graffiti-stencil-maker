import { useEffect, useRef, useState } from 'react'
import * as THREE from 'three'
import { OrbitControls } from 'three/examples/jsm/controls/OrbitControls.js'
import { STLLoader } from 'three/examples/jsm/loaders/STLLoader.js'
import { SimplifyModifier } from 'three/examples/jsm/modifiers/SimplifyModifier.js'

export interface Part { name: string; url: string }
/** The plate description a Tiles node (and Pads node) attaches to its parts. */
export interface PlateMeta {
  w: number; h: number
  parts: { file: string; origin: [number, number]; size: [number, number] }[]
  connectors?: number[][]
  pads?: { x: number; y: number; r: number }[]
  holes?: { x: number; y: number; r: number }[]
  joins?: { a: [number, number]; b: [number, number] }[]
}
interface Scene3D {
  meshes: THREE.Mesh[]
  geos: { hi: THREE.BufferGeometry; lo?: THREE.BufferGeometry }[]
  centers: THREE.Vector3[]
  boxes: THREE.Box3[]
  center: THREE.Vector3
  box: THREE.Box3
  camera: THREE.PerspectiveCamera
  controls: OrbitControls
  r: number
}

/** Orbitable STL viewer: wireframe, Z-clip (scrub through layers) and an exploded view for tiled parts. */
// the camera survives regenerations so a finished run doesn't reset the user's view
let savedCam: { pos: THREE.Vector3; target: THREE.Vector3 } | null = null

export default function Viewer3D({ parts, plate }: { parts: Part[]; plate?: PlateMeta | null }) {
  const host = useRef<HTMLDivElement>(null)
  const sc = useRef<Scene3D | null>(null)
  const [wire, setWire] = useState(false)
  const [shading, setShading] = useState<'smooth' | 'flat'>('smooth')
  const [detail, setDetail] = useState<'auto' | 'high' | 'low'>('auto')
  const [interacting, setInteracting] = useState(false)
  const [explode, setExplode] = useState(0)
  const [hidden, setHidden] = useState<Set<string>>(new Set())
  const [clip, setClip] = useState(1)
  const [layout, setLayout] = useState(true)
  const [status, setStatus] = useState('loading…')
  const [ready, setReady] = useState(0)
  const [dims, setDims] = useState<{ total: string; parts: Record<string, string> }>({ total: '', parts: {} })
  const urls = parts.map((p) => p.url).join('|')
  const tileRe = /_r(\d+)c(\d+)\.stl$/
  const isTile = (n: string) => tileRe.test(n)
  const tiled = parts.some((p) => isTile(p.name))
  const colorOf = (i: number) => (parts.length > 1 ? new THREE.Color().setHSL((i * 0.137) % 1, 0.55, 0.62) : new THREE.Color(0xffffff))
  useEffect(() => { setHidden(new Set()); setExplode(tiled ? 12 : 0) }, [urls])

  useEffect(() => {
    const el = host.current!
    let renderer: THREE.WebGLRenderer
    try {
      renderer = new THREE.WebGLRenderer({ antialias: true })
    } catch {
      setStatus('WebGL is unavailable in this browser: use the download buttons')
      return
    }
    renderer.localClippingEnabled = true
    const scene = new THREE.Scene()
    scene.background = new THREE.Color(0x16191e)
    const camera = new THREE.PerspectiveCamera(40, 1, 0.1, 1e6)
    camera.up.set(0, 0, 1)
    scene.add(new THREE.HemisphereLight(0xffffff, 0x334455, 1.1))
    const dl = new THREE.DirectionalLight(0xffffff, 1.6)
    dl.position.set(-1, -1.5, 2)
    scene.add(dl)
    el.appendChild(renderer.domElement)
    const controls = new OrbitControls(camera, renderer.domElement)
    let idle: number | null = null
    controls.addEventListener('start', () => setInteracting(true))
    controls.addEventListener('end', () => {
      if (idle) window.clearTimeout(idle)
      idle = window.setTimeout(() => setInteracting(false), 80)
    })
    controls.addEventListener('change', () => { savedCam = { pos: camera.position.clone(), target: controls.target.clone() } })
    const ro = new ResizeObserver(() => {
      const { clientWidth: w, clientHeight: h } = el
      renderer.setSize(w, h)
      camera.aspect = w / Math.max(h, 1)
      camera.updateProjectionMatrix()
    })
    ro.observe(el)
    let alive = true
    setStatus('loading…')
    const loader = new STLLoader()
    Promise.all(parts.map((p) => loader.loadAsync(p.url))).then((geos) => {
      if (!alive) return
      const box = new THREE.Box3()
      const group = new THREE.Group()
      const meshes: THREE.Mesh[] = []
      const geoms: { hi: THREE.BufferGeometry; lo?: THREE.BufferGeometry }[] = []
      const centers: THREE.Vector3[] = []
      const boxes: THREE.Box3[] = []
      const tris = geos.reduce((a, g) => a + (g.index ? g.index.count : g.attributes.position.count) / 3, 0)
      const wantLod = tris > 200_000
      const mod = wantLod ? new SimplifyModifier() : null
      geos.forEach((raw, i) => {
        const hi = raw.clone()
        hi.computeVertexNormals()
        hi.computeBoundingBox()
        box.union(hi.boundingBox!)
        centers.push(hi.boundingBox!.getCenter(new THREE.Vector3()))
        boxes.push(hi.boundingBox!.clone())
        let lo: THREE.BufferGeometry | undefined
        if (mod) {
          const src = hi.index ? hi.toNonIndexed() : hi.clone()
          const n = src.attributes.position.count
          const keep = Math.max(300, Math.floor(n * 0.35))
          const remove = Math.max(0, n - keep)
          if (remove > 0) {
            lo = mod.modify(src, remove)
            lo.computeVertexNormals()
          }
        }
        geoms.push({ hi, lo })
        const color = colorOf(i)
        const m = new THREE.Mesh(hi, new THREE.MeshStandardMaterial({ color, roughness: 0.6, metalness: 0.1 }))
        meshes.push(m)
        group.add(m)
      })
      const center = box.getCenter(new THREE.Vector3())
      group.position.copy(center).negate()
      scene.add(group)
      const r = box.getSize(new THREE.Vector3()).length() / 2
      if (savedCam) {
        camera.position.copy(savedCam.pos)
        controls.target.copy(savedCam.target)
      } else camera.position.set(0, -r * 2.2, r * 1.6)
      controls.update()
      sc.current = { meshes, geos: geoms, centers, boxes, center, box, camera, controls, r }
      setStatus('')
      setReady((n) => n + 1)
    }).catch((e) => alive && setStatus(`could not load the mesh: ${e?.message ?? e}`))
    let raf = 0
    const tick = () => { raf = requestAnimationFrame(tick); controls.update(); renderer.render(scene, camera) }
    tick()
    return () => {
      alive = false
      sc.current?.geos.forEach((g) => { g.hi.dispose(); g.lo?.dispose() })
      sc.current = null
      cancelAnimationFrame(raf)
      ro.disconnect()
      renderer.dispose()
      el.replaceChildren()
    }
  }, [urls])

  useEffect(() => {
    const s = sc.current
    if (!s) return
    const low = detail === 'low' || (detail === 'auto' && interacting)
    const zmin = s.box.min.z - s.center.z, h = s.box.max.z - s.box.min.z
    const planes = clip < 1 ? [new THREE.Plane(new THREE.Vector3(0, 0, -1), zmin + clip * h + 1e-4)] : []
    const info = parts.map((p) => p.name.match(tileRe))
    const tiles = info.flatMap((m, i) => (m ? [i] : []))
    // each tile STL sits at its own origin: place it by the summed widths/heights of the tiles before it plus the gap
    const rowSet = [...new Set(tiles.map((i) => +info[i]![1]))].sort((x, y) => x - y)
    const colSet = [...new Set(tiles.map((i) => +info[i]![2]))].sort((x, y) => x - y)
    const colW = new Map<number, number>(), rowH = new Map<number, number>()
    tiles.forEach((i) => {
      const r = +info[i]![1], c = +info[i]![2]
      colW.set(c, Math.max(colW.get(c) ?? 0, s.boxes[i].max.x))
      rowH.set(r, Math.max(rowH.get(r) ?? 0, s.boxes[i].max.y))
    })
    const colX = new Map<number, number>(), rowY = new Map<number, number>()
    let acc = 0
    colSet.forEach((c) => { colX.set(c, acc); acc += colW.get(c)! + explode })
    acc = 0
    ;[...rowSet].reverse().forEach((r) => { rowY.set(r, acc); acc += rowH.get(r)! + explode })
    // loose parts (joiner/extender plates, rods) lay out in a row below the tiles
    let ax = 0
    const ay = -15
    const loose = parts.map((_, i) => i).filter((i) => tiles.length && !info[i]).sort((a, b) => parts[a].name.localeCompare(parts[b].name))
        s.meshes.forEach((m, i) => {
      const mat = m.material as THREE.MeshStandardMaterial
      const g = s.geos[i]
      m.geometry = low && g.lo ? g.lo : g.hi
      mat.wireframe = wire
      mat.flatShading = shading === 'flat'
      mat.clippingPlanes = planes
      mat.side = clip < 1 ? THREE.DoubleSide : THREE.FrontSide
      mat.needsUpdate = true
      m.visible = !hidden.has(parts[i].name)
      const t = info[i]
      if (t) m.position.set(colX.get(+t[2])!, rowY.get(+t[1])!, 0)
      else if (tiles.length) m.position.set(0, 0, 0)
      else m.position.set((s.centers[i].x - s.center.x) * explode / 12, (s.centers[i].y - s.center.y) * explode / 12, 0)
    })
    const fmt = (x: number, y: number, z: number) => `${x.toFixed(1)} × ${y.toFixed(1)} × ${z.toFixed(1)} mm`
    const partDims: Record<string, string> = {}
    s.boxes.forEach((b, i) => { const d = b.getSize(new THREE.Vector3()); partDims[parts[i].name] = fmt(d.x, d.y, d.z) })
    let total: string
    if (tiles.length) {
      // assembled plate: tiles side by side without spacing
      const zmax = Math.max(...tiles.map((i) => s.boxes[i].getSize(new THREE.Vector3()).z))
      total = fmt([...colW.values()].reduce((a, b) => a + b, 0), [...rowH.values()].reduce((a, b) => a + b, 0), zmax)
    } else {
      const d = s.box.getSize(new THREE.Vector3())
      total = fmt(d.x, d.y, d.z)
    }
    setDims((o) => (o.total === total && JSON.stringify(o.parts) === JSON.stringify(partDims) ? o : { total, parts: partDims }))
    loose.forEach((i) => {
      const sz = s.boxes[i].getSize(new THREE.Vector3())
      s.meshes[i].position.set(ax + sz.x / 2 - s.centers[i].x, ay - sz.y / 2 - s.centers[i].y, -s.boxes[i].min.z)
      ax += sz.x + 8
    })
  }, [wire, shading, detail, interacting, explode, clip, ready, hidden])

  // seams, connectors, pads, holes and joiner lines drawn on each tile, so they follow it as the spacing changes
  useEffect(() => {
    const s = sc.current
    if (!s || !plate) return
    const made: { owner: THREE.Object3D; obj: THREE.Object3D }[] = []
    const mat = (c: number) => new THREE.LineBasicMaterial({ color: c, depthTest: false, transparent: true })
    const seamMat = mat(0xffc400), connMat = mat(0x4fc3f7), padMat = mat(0x66ff66), holeMat = mat(0xff6e6e), joinMat = mat(0xd081ff)
    const ring = (cx: number, cy: number, r: number, z: number, m: THREE.Material) => {
      const pts: THREE.Vector3[] = []
      for (let i = 0; i < 40; i++) pts.push(new THREE.Vector3(cx + r * Math.cos((i / 40) * 2 * Math.PI), cy + r * Math.sin((i / 40) * 2 * Math.PI), z))
      return new THREE.LineLoop(new THREE.BufferGeometry().setFromPoints(pts), m)
    }
    const poly = (pts: [number, number][], z: number, m: THREE.Material) => new THREE.LineLoop(new THREE.BufferGeometry().setFromPoints(pts.map(([x, y]) => new THREE.Vector3(x, y, z))), m)
    const seg = (a: [number, number], b: [number, number], z: number, m: THREE.Material) => new THREE.Line(new THREE.BufferGeometry().setFromPoints([new THREE.Vector3(a[0], a[1], z), new THREE.Vector3(b[0], b[1], z)]), m)
    const lox = Math.min(...plate.parts.map((p) => p.origin[0])), loy = Math.min(...plate.parts.map((p) => p.origin[1]))
    const hix = lox + plate.w, hiy = loy + plate.h
    parts.forEach((p, i) => {
      const t = plate.parts.find((q) => q.file === p.name)
      if (!t) return
      const [ox, oy] = t.origin, [w, h] = t.size
      const z = s.boxes[i].max.z + 0.05
      const add = (obj: THREE.Object3D) => { s.meshes[i].add(obj); made.push({ owner: s.meshes[i], obj }) }
      const near = (v: number, edge: number) => Math.abs(v - edge) < 0.01
      // only the borders that are seams, not the outer edge of the plate
      const edges: [boolean, [number, number], [number, number]][] = [
        [!near(oy, loy), [0, 0], [w, 0]], [!near(oy + h, hiy), [0, h], [w, h]],
        [!near(ox, lox), [0, 0], [0, h]], [!near(ox + w, hix), [w, 0], [w, h]],
      ]
      edges.forEach(([on, a, b]) => on && add(seg(a, b, z, seamMat)))
      const inside = (x: number, y: number) => x >= ox && x <= ox + w && y >= oy && y <= oy + h
      for (const c of plate.connectors ?? []) {
        const x0 = Math.max(c[0], ox), y0 = Math.max(c[1], oy), x1 = Math.min(c[2], ox + w), y1 = Math.min(c[3], oy + h)
        if (x0 < x1 && y0 < y1) add(poly([[x0 - ox, y0 - oy], [x1 - ox, y0 - oy], [x1 - ox, y1 - oy], [x0 - ox, y1 - oy]], z, connMat))
      }
      for (const q of plate.pads ?? []) if (inside(q.x, q.y)) add(ring(q.x - ox, q.y - oy, q.r, z, padMat))
      for (const q of plate.holes ?? []) if (inside(q.x, q.y)) add(ring(q.x - ox, q.y - oy, q.r, z, holeMat))
      for (const j of plate.joins ?? []) {
        // the joiner plate spans a seam: draw its line, clipped to this tile
        let t0 = 0, t1 = 1
        for (const [k, lo, hi] of [[0, ox, ox + w], [1, oy, oy + h]] as const) {
          const d = j.b[k] - j.a[k]
          if (Math.abs(d) < 1e-9) { if (j.a[k] < lo || j.a[k] > hi) t1 = -1; continue }
          const [u, v] = [(lo - j.a[k]) / d, (hi - j.a[k]) / d]
          t0 = Math.max(t0, Math.min(u, v)); t1 = Math.min(t1, Math.max(u, v))
        }
        if (t0 < t1) {
          const at = (t: number): [number, number] => [j.a[0] + (j.b[0] - j.a[0]) * t - ox, j.a[1] + (j.b[1] - j.a[1]) * t - oy]
          add(seg(at(t0), at(t1), z, joinMat))
        }
      }
    })
    made.forEach((m) => { m.obj.visible = layout })
    return () => made.forEach((m) => { m.owner.remove(m.obj); (m.obj as THREE.Line).geometry?.dispose() })
  }, [ready, plate, layout])

  // refit the camera so every part stays inside the panel as the spacing changes
  const fitRef = useRef(-1)
  useEffect(() => {
    const s = sc.current
    if (!s) return
    const changed = fitRef.current !== explode
    fitRef.current = explode
    if (!changed && savedCam) return
    const b = new THREE.Box3()
    s.meshes.forEach((m) => { if (m.visible) { m.updateMatrixWorld(true); b.union(new THREE.Box3().setFromObject(m)) } })
    if (b.isEmpty()) return
    const c = b.getCenter(new THREE.Vector3()), R = b.getSize(new THREE.Vector3()).length() / 2
    const dir = s.camera.position.clone().sub(s.controls.target)
    if (dir.length() < 1e-6) return
    const d = Math.max(R * 2.8, s.r * 0.5)
    s.controls.target.copy(c)
    s.camera.position.copy(c).add(dir.setLength(d))
    s.controls.update()
  }, [explode, ready])

  const view = (v: 'iso' | 'top' | 'bottom' | 'left' | 'right') => {
    const s = sc.current
    if (!s) return
    const e = s.r * 0.001, d = s.r * 3
    const pos: Record<string, [number, number, number]> = {
      iso: [0, -s.r * 2.2, s.r * 1.6], top: [0, -e, d], bottom: [0, -e, -d], left: [-d, 0, e], right: [d, 0, e],
    }
    s.camera.position.set(...pos[v])
    s.controls.target.set(0, 0, 0)
    s.controls.update()
  }
  return (
    <div className="v3">
      <div className="v3bar">
        <button onClick={() => view('iso')}>Iso</button>
        <button onClick={() => view('top')}>Top</button>
        <button onClick={() => view('bottom')}>Bottom</button>
        <button onClick={() => view('left')}>Left</button>
        <button onClick={() => view('right')}>Right</button>
        <label><input type="checkbox" checked={wire} onChange={(e) => setWire(e.target.checked)} /> wireframe</label>
        <label>shading <select value={shading} onChange={(e) => setShading(e.target.value as 'smooth' | 'flat')}><option value="smooth">smooth</option><option value="flat">flat</option></select></label>
        <label>detail <select value={detail} onChange={(e) => setDetail(e.target.value as 'auto' | 'high' | 'low')}><option value="auto">auto</option><option value="high">high</option><option value="low">low</option></select></label>
        {plate && <label title="seams (yellow), connectors (blue), pads (green), holes (red), joiner plates (purple)"><input type="checkbox" checked={layout} onChange={(e) => setLayout(e.target.checked)} /> layout</label>}
        <label>layers up to <input type="range" min={0.01} max={1} step={0.01} value={clip} onChange={(e) => setClip(+e.target.value)} /></label>
        {parts.length > 1 && <label>{tiled ? 'spacing (mm)' : 'explode'} <input type="range" min={0} max={tiled ? 150 : 12} step={tiled ? 1 : 0.25} value={explode} onChange={(e) => setExplode(+e.target.value)} /></label>}
        <span className="mut">{parts.length > 1 ? `${parts.length} parts` : ''}</span>
      </div>
      <div className="v3main">
        <div className="v3host">
          <div className="viewer" ref={host} />
          {status && <div className="v3status">{status}</div>}
          {dims.total && !status && <div className="v3dims" title="width × depth × height of the model">{tiled ? 'Assembled' : parts.length > 1 ? 'Overall' : 'Size'}: <b>{dims.total}</b></div>}
        </div>
        {parts.length > 1 && (
          <aside className="v3parts">
            <div className="v3parts-head"><b>Files</b><button onClick={() => setHidden(new Set())}>all</button><button onClick={() => setHidden(new Set(parts.map((p) => p.name)))}>none</button></div>
            {parts.map((p, i) => (
              <div key={p.name} className={hidden.has(p.name) ? 'v3part off' : 'v3part'}>
                <label title={p.name}><input type="checkbox" checked={!hidden.has(p.name)} onChange={() => setHidden((h) => { const n = new Set(h); n.has(p.name) ? n.delete(p.name) : n.add(p.name); return n })} />
                  <i style={{ background: `#${colorOf(i).getHexString()}` }} />{p.name.replace(/\.stl$/, '')}</label>
                {dims.parts[p.name] && <span className="v3size">{dims.parts[p.name]}</span>}
                <button title="show only this part" onClick={() => setHidden(new Set(parts.filter((q) => q.name !== p.name).map((q) => q.name)))}>only</button>
                <a className="btn" href={p.url} download={p.name.endsWith('.stl') ? p.name : `${p.name}.stl`} title="download this STL">⬇</a>
              </div>
            ))}
          </aside>
        )}
      </div>
    </div>
  )
}
