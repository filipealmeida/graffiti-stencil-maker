import { useEffect, useRef } from 'react'
import * as THREE from 'three'
import { OrbitControls } from 'three/examples/jsm/controls/OrbitControls.js'
import { STLLoader } from 'three/examples/jsm/loaders/STLLoader.js'

export default function Viewer3D({ url }: { url: string }) {
  const host = useRef<HTMLDivElement>(null)
  useEffect(() => {
    const el = host.current!
    let renderer: THREE.WebGLRenderer
    try {
      renderer = new THREE.WebGLRenderer({ antialias: true })
    } catch {
      el.textContent = 'WebGL unavailable: use the download button'
      return
    }
    const scene = new THREE.Scene()
    scene.background = new THREE.Color(0x16191e)
    const camera = new THREE.PerspectiveCamera(40, 1, 0.1, 100000)
    camera.up.set(0, 0, 1)
    scene.add(new THREE.HemisphereLight(0xffffff, 0x334455, 1.1))
    const dl = new THREE.DirectionalLight(0xffffff, 1.6)
    dl.position.set(-1, -1.5, 2)
    scene.add(dl)
    el.appendChild(renderer.domElement)
    const controls = new OrbitControls(camera, renderer.domElement)
    const resize = () => {
      const { clientWidth: w, clientHeight: h } = el
      renderer.setSize(w, h)
      camera.aspect = w / Math.max(h, 1)
      camera.updateProjectionMatrix()
    }
    new ResizeObserver(resize).observe(el)
    resize()
    let alive = true
    new STLLoader().load(url, (geo) => {
      if (!alive) return
      geo.computeVertexNormals()
      geo.computeBoundingBox()
      const c = geo.boundingBox!.getCenter(new THREE.Vector3())
      geo.translate(-c.x, -c.y, -c.z)
      scene.add(new THREE.Mesh(geo, new THREE.MeshStandardMaterial({ color: 0xffffff, roughness: 0.6, metalness: 0.1 })))
      const r = geo.boundingBox!.getSize(new THREE.Vector3()).length() / 2
      camera.position.set(0, -r * 2.2, r * 1.6)
      controls.update()
    })
    let raf = 0
    const tick = () => { raf = requestAnimationFrame(tick); controls.update(); renderer.render(scene, camera) }
    tick()
    return () => { alive = false; cancelAnimationFrame(raf); renderer.dispose(); el.replaceChildren() }
  }, [url])
  return <div className="viewer" ref={host} />
}
