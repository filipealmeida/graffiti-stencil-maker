import { useEffect, useRef, useState } from 'react'
import * as THREE from 'three'
import { OrbitControls } from 'three/examples/jsm/controls/OrbitControls.js'
import { STLLoader } from 'three/examples/jsm/loaders/STLLoader.js'

export interface Part { name: string; url: string }
interface Scene3D {
  meshes: THREE.Mesh[]; centers: THREE.Vector3[]; boxes: THREE.Box3[]; center: THREE.Vector3; box: THREE.Box3
  camera: THREE.PerspectiveCamera; controls: OrbitControls; r: number
}

/** Orbitable STL viewer: wireframe, Z-clip (scrub through layers) and an exploded view for tiled parts. */
// the camera survives regenerations so a finished run doesn't reset the user's view
let savedCam: { pos: THREE.Vector3; target: THREE.Vector3 } | null = null

export default function Viewer3D({ parts }: { parts: Part[] }) {
  const host = useRef<HTMLDivElement>(null)
  const sc = useRef<Scene3D | null>(null)
  const [wire, setWire] = useState(false)
  const [explode, setExplode] = useState(0)
  const [hidden, setHidden] = useState<Set<string>>(new Set())
  const [clip, setClip] = useState(1)
  const [status, setStatus] = useState('loading…')
  const [ready, setReady] = useState(0)
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
      const meshes: THREE.Mesh[] = [], centers: THREE.Vector3[] = [], boxes: THREE.Box3[] = []
      geos.forEach((geo, i) => {
        geo.computeVertexNormals()
        geo.computeBoundingBox()
        box.union(geo.boundingBox!)
        centers.push(geo.boundingBox!.getCenter(new THREE.Vector3()))
        boxes.push(geo.boundingBox!.clone())
        const color = colorOf(i)
        const m = new THREE.Mesh(geo, new THREE.MeshStandardMaterial({ color, roughness: 0.6, metalness: 0.1 }))
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
      sc.current = { meshes, centers, boxes, center, box, camera, controls, r }
      setStatus('')
      setReady((n) => n + 1)
    }).catch((e) => alive && setStatus(`could not load the mesh: ${e?.message ?? e}`))
    let raf = 0
    const tick = () => { raf = requestAnimationFrame(tick); controls.update(); renderer.render(scene, camera) }
    tick()
    return () => { alive = false; sc.current = null; cancelAnimationFrame(raf); ro.disconnect(); renderer.dispose(); el.replaceChildren() }
  }, [urls])

  useEffect(() => {
    const s = sc.current
    if (!s) return
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
      mat.wireframe = wire
      mat.clippingPlanes = planes
      mat.side = clip < 1 ? THREE.DoubleSide : THREE.FrontSide
      mat.needsUpdate = true
      m.visible = !hidden.has(parts[i].name)
      const t = info[i]
      if (t) m.position.set(colX.get(+t[2])!, rowY.get(+t[1])!, 0)
      else if (tiles.length) m.position.set(0, 0, 0)
      else m.position.set((s.centers[i].x - s.center.x) * explode / 12, (s.centers[i].y - s.center.y) * explode / 12, 0)
    })
    loose.forEach((i) => {
      const sz = s.boxes[i].getSize(new THREE.Vector3())
      s.meshes[i].position.set(ax + sz.x / 2 - s.centers[i].x, ay - sz.y / 2 - s.centers[i].y, -s.boxes[i].min.z)
      ax += sz.x + 8
    })
  }, [wire, explode, clip, ready, hidden])

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
        <label>layers up to <input type="range" min={0.01} max={1} step={0.01} value={clip} onChange={(e) => setClip(+e.target.value)} /></label>
        {parts.length > 1 && <label>{tiled ? 'spacing (mm)' : 'explode'} <input type="range" min={0} max={tiled ? 150 : 12} step={tiled ? 1 : 0.25} value={explode} onChange={(e) => setExplode(+e.target.value)} /></label>}
        <span className="mut">{parts.length > 1 ? `${parts.length} parts` : ''}</span>
      </div>
      <div className="v3main">
        <div className="v3host">
          <div className="viewer" ref={host} />
          {status && <div className="v3status">{status}</div>}
        </div>
        {parts.length > 1 && (
          <aside className="v3parts">
            <div className="v3parts-head"><b>Files</b><button onClick={() => setHidden(new Set())}>all</button><button onClick={() => setHidden(new Set(parts.map((p) => p.name)))}>none</button></div>
            {parts.map((p, i) => (
              <div key={p.name} className={hidden.has(p.name) ? 'v3part off' : 'v3part'}>
                <label title={p.name}><input type="checkbox" checked={!hidden.has(p.name)} onChange={() => setHidden((h) => { const n = new Set(h); n.has(p.name) ? n.delete(p.name) : n.add(p.name); return n })} />
                  <i style={{ background: `#${colorOf(i).getHexString()}` }} />{p.name.replace(/\.stl$/, '')}</label>
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
