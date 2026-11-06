import { useEffect, useState } from 'react'
import { STORE_KEY, useStudio, type StudioNode } from './store'
import type { Edge } from '@xyflow/react'
import { validateGraphSnapshot } from './projectSchema'

export type AutosaveStatus = { state: 'idle' | 'saving' | 'saved' | 'error'; time?: string; error?: string }

interface AutosaveDraft {
  version: 2
  savedAt: string
  nodes: StudioNode[]
  edges: Edge[]
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
}

/** Reads the current versioned draft and accepts the legacy unversioned v1 snapshot. */
export function readAutosave(): { nodes: StudioNode[]; edges: Edge[]; savedAt?: string } | null {
  const text = localStorage.getItem(STORE_KEY)
  if (!text) return null
  let value: unknown
  try {
    value = JSON.parse(text)
  } catch {
    throw new Error('The saved browser draft is not valid JSON. Export it from the recovery screen before clearing it.')
  }
  if (!isRecord(value)) throw new Error('The saved browser draft has an invalid shape.')
  if (value.version !== undefined && value.version !== 2)
    throw new Error(`The saved browser draft uses an unsupported version (${String(value.version)}).`)
  const graph = validateGraphSnapshot(value)
  return { ...graph, savedAt: typeof value.savedAt === 'string' ? value.savedAt : undefined }
}

/** Debounced, versioned local recovery snapshot; separate from explicit project-file saving. */
export default function useProjectAutosave(enabled: boolean): AutosaveStatus {
  const nodes = useStudio((state) => state.nodes)
  const edges = useStudio((state) => state.edges)
  const [status, setStatus] = useState<AutosaveStatus>({ state: 'idle' })

  useEffect(() => {
    if (!enabled) return
    setStatus({ state: 'saving' })
    const save = () => {
      try {
        const savedAt = new Date().toISOString()
        const current = useStudio.getState()
        const draft: AutosaveDraft = { version: 2, savedAt, nodes: current.nodes, edges: current.edges }
        localStorage.setItem(STORE_KEY, JSON.stringify(draft))
        setStatus({ state: 'saved', time: new Date(savedAt).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' }) })
      } catch (error) {
        setStatus({ state: 'error', error: error instanceof Error ? error.message : String(error) })
      }
    }
    const timer = window.setTimeout(save, 700)
    const flush = () => {
      window.clearTimeout(timer)
      save()
    }
    window.addEventListener('pagehide', flush)
    return () => {
      window.clearTimeout(timer)
      window.removeEventListener('pagehide', flush)
    }
  }, [enabled, nodes, edges])

  return status
}
