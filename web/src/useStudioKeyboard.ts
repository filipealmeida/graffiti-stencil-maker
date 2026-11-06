import { useEffect, useRef } from 'react'
import { redo, rerun, stopRunning, undo, useStudio } from './store'
import { reportError } from './store'
import { saveProject } from './project'

/** Global studio shortcuts, kept separate from the canvas orchestration. */
export default function useStudioKeyboard(
  closeMenus: () => void,
  toggleCommandPalette: () => void,
) {
  const actions = useRef({ closeMenus, toggleCommandPalette })
  actions.current = { closeMenus, toggleCommandPalette }

  useEffect(() => {
    const keydown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        const menuOpen = !!document.querySelector('.menu, .cmdpal')
        actions.current.closeMenus()
        if (menuOpen) return
        const st = useStudio.getState()
        if (st.bench) st.openBench(null)
        else if (st.selected && !(e.target as HTMLElement).closest?.('input, textarea, select')) st.select(null)
        return
      }

      const target = e.target as HTMLElement
      const key = e.key.toLowerCase()
      if ((e.ctrlKey || e.metaKey) && (key === 's' || key === 'o')) {
        e.preventDefault()
        if (key === 's') void saveProject().catch(reportError)
        else document.getElementById('open-project')?.click()
        return
      }
      if (target.closest?.('input, textarea, select, [contenteditable]') || !(e.ctrlKey || e.metaKey)) return

      if (key === 'z' && !e.shiftKey) {
        e.preventDefault()
        undo()
      } else if ((key === 'z' && e.shiftKey) || key === 'y') {
        e.preventDefault()
        redo()
      } else if (key === 'c') {
        useStudio.getState().copy()
      } else if (key === 'v') {
        e.preventDefault()
        useStudio.getState().paste()
      } else if (key === 'd') {
        e.preventDefault()
        useStudio.getState().duplicate()
      } else if (key === '.') {
        e.preventDefault()
        stopRunning()
      } else if (key === 'enter') {
        e.preventDefault()
        rerun()
      } else if (key === 'k') {
        e.preventDefault()
        actions.current.toggleCommandPalette()
      }
    }

    window.addEventListener('keydown', keydown)
    return () => window.removeEventListener('keydown', keydown)
  }, [])
}
