import { api } from './api'
import { useStudio } from './store'

/** Replace the picture loaded in an Image node, or save the one loaded. */
export default function SourceActions({ id, imageId, title }: { id: string; imageId: string; title?: string }) {
  const open = async (f: File) => {
    try {
      const img = await api.upload(f)
      // one update, so a single undo restores both the picture and its title
      useStudio.setState((st) => ({ nodes: st.nodes.map((n) => (n.id === id ? { ...n, data: { ...n.data, title: img.name, params: { ...n.data.params, image_id: img.id } } } : n)) }))
    } catch (e) {
      useStudio.setState({ error: String(e instanceof Error ? e.message : e) })
    }
  }
  const download = async () => {
    const img = await api.storedImage(imageId)
    if (!img) return useStudio.setState({ error: 'the picture is no longer stored in this browser' })
    const url = URL.createObjectURL(new Blob([img.bytes]))
    const a = document.createElement('a')
    a.href = url
    a.download = img.name || title || 'image'
    a.click()
    setTimeout(() => URL.revokeObjectURL(url), 10_000)
  }
  return (
    <div className="node-actions">
      <label className="btn" title="Replace the picture loaded in this node">Open…
        <input type="file" accept=".png,.jpg,.jpeg,.bmp,.svg" hidden onChange={(e) => { const f = e.target.files?.[0]; e.target.value = ''; if (f) open(f) }} />
      </label>
      <button title="Download the original picture loaded in this node" onClick={download}>Download</button>
    </div>
  )
}
