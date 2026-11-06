import { api } from './api'
import { checkpoint, logAction, reportError, undo, useStudio } from './store'
import Tooltip from './Tooltip'
import messages from './messages'

/** Replace or download the source file of an Image or 3D import node. */
export default function SourceActions({ id, imageId, title, model = false }: { id: string; imageId: string; title?: string; model?: boolean }) {
  const noun = model ? 'model' : 'picture'
  const open = async (f: File) => {
    logAction(`Opening ${noun}: ${f.name}`, id)
    try {
      const img = model ? await api.uploadModel(f) : await api.upload(f)
      // One update lets undo restore both the file and its title.
      const params = 'format' in img ? { model_id: img.id, format: img.format } : { image_id: img.id }
      checkpoint()
      useStudio.setState((st) => ({ nodes: st.nodes.map((n) => (n.id === id ? { ...n, data: { ...n.data, title: img.name, params: { ...n.data.params, ...params } } } : n)) }))
      checkpoint()
      useStudio.getState().toast(`Replaced ${noun} with ${img.name}`, { undo })
      const info = 'format' in img ? await api.modelInfo(img.id, img.format) : null
      logAction(`Opened ${img.name}${info ? `: ${info.triangles} triangles; ${info.valid ? 'solid checks passed' : 'defects found, solid output blocked; inspect the repair tools'}` : ''}`, id)
    } catch (e) {
      reportError(e, id)
    }
  }
  const download = async () => {
    try {
      const img = await api.storedImage(imageId)
      if (!img) return reportError(`the ${noun} is no longer stored in this browser`, id)
      const url = URL.createObjectURL(new Blob([img.bytes]))
      const a = document.createElement('a')
      a.href = url
      a.download = img.name || title || noun
      a.click()
      logAction(`Source download handed to browser: ${a.download}`, id)
      setTimeout(() => URL.revokeObjectURL(url), 10_000)
    } catch (e) {
      reportError(e, id)
    }
  }
  return (
    <div className="node-actions">
      <Tooltip content={model ? messages.actions.replaceModel : messages.actions.replacePicture}><label className="btn">Open…
        <input type="file" accept={model ? '.stl,.obj' : '.png,.jpg,.jpeg,.bmp,.svg'} hidden onChange={(e) => { const f = e.target.files?.[0]; e.target.value = ''; if (f) open(f) }} />
      </label></Tooltip>
      <Tooltip content={model ? 'Download the current model source (including applied repairs)' : 'Download the original picture loaded in this node'}><button disabled={!imageId} onClick={download}>Download</button></Tooltip>
    </div>
  )
}
