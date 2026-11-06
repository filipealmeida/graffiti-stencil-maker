import { useCallback } from 'react'
import { api } from './api'
import { forgetFile } from './project'
import { checkpoint, logAction, reportError, undo, useStudio } from './store'

/** Project and source-file operations, separated from canvas rendering/orchestration. */
export default function useProjectIO(fit: () => void) {
  const openProject = useCallback(async (file: File) => {
    logAction(`Opening project: ${file.name}`)
    try {
      const graph = await api.importProject(JSON.parse(await file.text()))
      const oldName = useStudio.getState().pname
      checkpoint()
      forgetFile()
      useStudio.getState().setPname(file.name.replace(/\.stencil\.json$|\.json$/, ''))
      useStudio.getState().load(graph.nodes, graph.edges)
      useStudio.getState().markSaved()
      checkpoint()
      fit()
      useStudio.getState().toast(`Opened ${file.name}`, { undo: () => { undo(); useStudio.getState().setPname(oldName) } })
    } catch (error) {
      reportError(`cannot open project: ${error instanceof Error ? error.message : String(error)}`)
    }
  }, [fit])

  const addFile = useCallback(async (file: File, at?: { x: number; y: number }) => {
    logAction(`Opening file: ${file.name}`)
    try {
      if (/\.(stl|obj)$/i.test(file.name)) {
        const model = await api.uploadModel(file)
        const id = useStudio.getState().addNode('mesh_import', { model_id: model.id, format: model.format }, at, model.name)
        const info = await api.modelInfo(model.id, model.format)
        logAction(`Opened ${model.name}: ${info.triangles} triangles; ${info.valid ? 'solid checks passed' : 'defects found; solid output blocked'}`, id)
        if (useStudio.getState().nodes.length === 1) fit()
        return
      }

      const image = await api.upload(file)
      const state = useStudio.getState()
      if (!state.nodes.length) {
        state.vectorStarter(image.id, image.name)
        fit()
      } else {
        const sources = state.nodes.filter((node) => node.data.type === 'source')
        if (sources.length === 1) {
          checkpoint()
          useStudio.setState((current) => ({
            nodes: current.nodes.map((node) => node.id === sources[0].id
              ? { ...node, data: { ...node.data, title: image.name, params: { ...node.data.params, image_id: image.id } } }
              : node),
          }))
          checkpoint()
          useStudio.getState().toast(`Replaced picture with ${image.name}`, { undo })
        } else {
          state.addNode('source', { image_id: image.id }, at, image.name)
        }
      }
      logAction(`Opened picture: ${image.name}`)
    } catch (error) {
      reportError(error)
    }
  }, [fit])

  return { openProject, addFile }
}
