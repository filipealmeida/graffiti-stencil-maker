import { useEffect, useState } from 'react'
import { api, type ImageInfo } from './api'

const size = (n: number) => (n < 1024 ? `${n} B` : n < 1048576 ? `${(n / 1024).toFixed(1)} KB` : `${(n / 1048576).toFixed(2)} MB`)

/** What the picture loaded in an Image node is, and how the pipeline will read it. */
export default function ImageInfoPanel({ imageId, resolution }: { imageId: string; resolution: number }) {
  const [info, setInfo] = useState<ImageInfo | null>(null)
  const [name, setName] = useState('')
  const [err, setErr] = useState('')
  useEffect(() => {
    let live = true
    setErr('')
    api.storedImage(imageId).then((s) => live && setName(s?.name ?? ''))
    api.imageInfo(imageId, resolution).then(
      (i) => live && setInfo(i),
      (e) => live && (setInfo(null), setErr(String(e instanceof Error ? e.message : e))),
    )
    return () => { live = false }
  }, [imageId, resolution])
  if (err) return <p className="mut">{err}</p>
  if (!info) return null
  const rows: [string, string][] = [
    ...(name ? ([['File', name]] as [string, string][]) : []),
    ['Format', info.format],
    ['File size', size(info.bytes)],
    ['Size', `${info.width} × ${info.height} px (${info.megapixels} MP)`],
    ['Aspect', String(info.aspect)],
    ...(info.colour_model ? ([['Colour', info.colour_model]] as [string, string][]) : []),
    ...(info.dpi ? ([['Resolution', `${info.dpi[0]} × ${info.dpi[1]} dpi`], ['Print size', `${info.print_size_mm![0]} × ${info.print_size_mm![1]} mm`]] as [string, string][]) : []),
    ...(info.svg_width || info.svg_height ? ([['SVG size', `${info.svg_width ?? '–'} × ${info.svg_height ?? '–'}`]] as [string, string][]) : []),
    ...(info.svg_viewbox ? ([['SVG viewBox', info.svg_viewbox]] as [string, string][]) : []),
    ...(info.svg_title ? ([['SVG title', info.svg_title]] as [string, string][]) : []),
    ['Working size', `${info.working[0]} × ${info.working[1]} px (${info.scale === 1 ? 'as is' : `${(info.scale * 100).toFixed(0)}%`})`],
    ['Brightness', `mean ${info.mean_brightness}, range ${info.brightness_range[0]}–${info.brightness_range[1]}`],
    ['Auto threshold', `${info.otsu_threshold} (${info.dark_percent}% would be paint)`],
    ['Colours', info.colours > 4096 ? 'more than 4096' : String(info.colours)],
  ]
  return (
    <section className="imageinfo" aria-label="Image properties">
      <h4>Image properties</h4>
      <dl>{rows.map(([k, v]) => (<div key={k} style={{ display: 'contents' }}><dt>{k}</dt><dd>{v}</dd></div>))}</dl>
    </section>
  )
}
