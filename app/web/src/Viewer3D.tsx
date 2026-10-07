import { useEffect, useRef, useState } from 'react'
import * as THREE from 'three'
import { OrbitControls } from 'three/examples/jsm/controls/OrbitControls.js'
import { STLLoader } from 'three/examples/jsm/loaders/STLLoader.js'

export interface Part { name: string; url: string }
interface Scene3D {
  meshes: THREE.Mesh[]; centers: THREE.Vector3[]; center: THREE.Vector3; box: THREE.Box3
  camera: THREE.PerspectiveCamera; controls: OrbitControls; r: number
}

/** Orbitable STL viewer: wireframe, Z-clip (scrub through layers) and an exploded view for tiled parts. */
export default function Viewer3D({ parts }: { parts: Part[] }) {
  const host = useRef<HTMLDivElement>(null)
  const sc = useRef<Scene3D | null>(null)
  const [wire, setWire] = useState(false)
  const [explode, setExplode] = useState(0)
  const [clip, setClip] = useState(1)
  const [status, setStatus] = useState('loading…')
  const [ready, setReady] = useState(0)
  const urls = parts.map((p) => p.url).join('|')

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
      const meshes: THREE.Mesh[] = [], centers: THREE.Vector3[] = []
      geos.forEach((geo, i) => {
        geo.computeVertexNormals()
        geo.computeBoundingBox()
        box.union(geo.boundingBox!)
        centers.push(geo.boundingBox!.getCenter(new THREE.Vector3()))
        const color = geos.length > 1 ? new THREE.Color().setHSL((i * 0.137) % 1, 0.55, 0.62) : new THREE.Color(0xffffff)
        const m = new THREE.Mesh(geo, new THREE.MeshStandardMaterial({ color, roughness: 0.6, metalness: 0.1 }))
        meshes.push(m)
        group.add(m)
      })
      const center = box.getCenter(new THREE.Vector3())
      group.position.copy(center).negate()
      scene.add(group)
      const r = box.getSize(new THREE.Vector3()).length() / 2
      camera.position.set(0, -r * 2.2, r * 1.6)
      controls.update()
      sc.current = { meshes, centers, center, box, camera, controls, r }
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
    s.meshes.forEach((m, i) => {
      const mat = m.material as THREE.MeshStandardMaterial
      mat.wireframe = wire
      mat.clippingPlanes = planes
      mat.side = clip < 1 ? THREE.DoubleSide : THREE.FrontSide
      mat.needsUpdate = true
      m.position.set((s.centers[i].x - s.center.x) * explode, (s.centers[i].y - s.center.y) * explode, 0)
    })
  }, [wire, explode, clip, ready])

  const view = (top: boolean) => {
    const s = sc.current
    if (!s) return
    s.camera.position.set(0, top ? -s.r * 0.001 : -s.r * 2.2, top ? s.r * 3 : s.r * 1.6)
    s.controls.target.set(0, 0, 0)
    s.controls.update()
  }
  return (
    <div className="v3">
      <div className="v3bar">
        <button onClick={() => view(false)}>Iso</button>
        <button onClick={() => view(true)}>Top</button>
        <label><input type="checkbox" checked={wire} onChange={(e) => setWire(e.target.checked)} /> wireframe</label>
        <label>layers up to <input type="range" min={0.01} max={1} step={0.01} value={clip} onChange={(e) => setClip(+e.target.value)} /></label>
        {parts.length > 1 && <label>explode <input type="range" min={0} max={1} step={0.02} value={explode} onChange={(e) => setExplode(+e.target.value)} /></label>}
        <span className="mut">{parts.length > 1 ? `${parts.length} parts` : ''}</span>
      </div>
      <div className="v3host">
        <div className="viewer" ref={host} />
        {status && <div className="v3status">{status}</div>}
      </div>
    </div>
  )
}
