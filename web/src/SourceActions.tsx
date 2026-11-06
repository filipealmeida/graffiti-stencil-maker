import { api } from './api'
import { useStudio } from './store'

/** Replace or download the source file of an Image or 3D import node. */
export default function SourceActions({ id, imageId, title, model = false }: { id: string; imageId: string; title?: string; model?: boolean }) {
  const noun = model ? 'model' : 'picture'
  const open = async (f: File) => {
    try {
      const img = model ? await api.uploadModel(f) : await api.upload(f)
      // One update lets undo restore both the file and its title.
      const params = 'format' in img ? { model_id: img.id, format: img.format } : { image_id: img.id }
      useStudio.setState((st) => ({ nodes: st.nodes.map((n) => (n.id === id ? { ...n, data: { ...n.data, title: img.name, params: { ...n.data.params, ...params } } } : n)) }))
    } catch (e) {
      useStudio.setState({ error: String(e instanceof Error ? e.message : e) })
    }
  }
  const download = async () => {
    try {
      const img = await api.storedImage(imageId)
      if (!img) return useStudio.setState({ error: `the ${noun} is no longer stored in this browser` })
      const url = URL.createObjectURL(new Blob([img.bytes]))
      const a = document.createElement('a')
      a.href = url
      a.download = img.name || title || noun
      a.click()
      setTimeout(() => URL.revokeObjectURL(url), 10_000)
    } catch (e) {
      useStudio.setState({ error: String(e instanceof Error ? e.message : e) })
    }
  }
  return (
    <div className="node-actions">
      <label className="btn" title={`Replace the ${noun} loaded in this node`}>Open…
        <input type="file" accept={model ? '.stl,.obj' : '.png,.jpg,.jpeg,.bmp,.svg'} hidden onChange={(e) => { const f = e.target.files?.[0]; e.target.value = ''; if (f) open(f) }} />
      </label>
      <button title={`Download the original ${noun} loaded in this node`} disabled={!imageId} onClick={download}>Download</button>
    </div>
  )
}
