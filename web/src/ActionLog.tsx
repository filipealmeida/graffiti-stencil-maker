import { useEffect } from 'react'
import { logAction, useStudio } from './store'

/** UI-only actions have no engine result. Record the activated control and its resulting state. */
export default function ActionLog() {
  useEffect(() => {
    const nodeOf = (element: Element) => element.closest('.react-flow__node')?.getAttribute('data-id') ?? useStudio.getState().bench ?? useStudio.getState().selected ?? ''
    const nameOf = (element: Element) => (element.getAttribute('aria-label') || element.textContent?.trim() || element.getAttribute('title') || element.tagName.toLowerCase()).replace(/\s+/g, ' ').slice(0, 120)
    let resizing: Element | null = null
    const resizeResult = (control: Element) => setTimeout(() => logAction(`${nameOf(control)}: ${control.getAttribute('aria-valuenow') ?? 'applied'}`, nodeOf(control)), 0)
    const down = (event: PointerEvent) => { resizing = event.target instanceof Element ? event.target.closest('[role=separator]') : null }
    const up = () => { if (resizing) resizeResult(resizing); resizing = null }
    const key = (event: KeyboardEvent) => {
      if (!event.key.startsWith('Arrow') || !(event.target instanceof Element)) return
      const control = event.target.closest('[role=separator]')
      if (control) resizeResult(control)
    }
    const click = (event: MouseEvent) => {
      if (!(event.target instanceof Element)) return
      const control = event.target.closest('button, a[download]')
      if (!control || control.matches(':disabled')) return
      const node = nodeOf(control)
      const name = nameOf(control)
      const file = control instanceof HTMLAnchorElement ? control.download : ''
      setTimeout(() => {
        if (file) logAction(`Download handed to browser: ${file} (browser does not report whether it was saved)`, node)
        else {
          const expanded = control.getAttribute('aria-expanded')
          const pressed = control.getAttribute('aria-pressed')
          logAction(`Control activated: ${name}${expanded !== null ? `; expanded=${expanded}` : ''}${pressed !== null ? `; pressed=${pressed}` : ''}`, node)
        }
      }, 0)
    }
    const change = (event: Event) => {
      const control = event.target
      if (!(control instanceof HTMLInputElement || control instanceof HTMLSelectElement || control instanceof HTMLTextAreaElement) || control.type === 'file') return
      const label = control.getAttribute('aria-label') || control.closest('label')?.textContent?.trim().replace(/\s+/g, ' ').slice(0, 100) || control.type
      const value = control instanceof HTMLInputElement && control.type === 'checkbox' ? String(control.checked) : control.value
      logAction(`${label}: ${value}`, nodeOf(control))
    }
    document.addEventListener('click', click, true)
    document.addEventListener('change', change)
    document.addEventListener('pointerdown', down, true)
    document.addEventListener('pointerup', up, true)
    document.addEventListener('keyup', key, true)
    return () => {
      document.removeEventListener('click', click, true)
      document.removeEventListener('change', change)
      document.removeEventListener('pointerdown', down, true)
      document.removeEventListener('pointerup', up, true)
      document.removeEventListener('keyup', key, true)
    }
  }, [])
  return null
}
