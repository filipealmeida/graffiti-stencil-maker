import { useEffect, type RefObject } from 'react'

/** Keeps keyboard focus in an active dialog and restores it when the dialog closes. */
export default function useDialogFocus<T extends HTMLElement>(
  ref: RefObject<T | null>,
  active = true,
) {
  useEffect(() => {
    if (!active) return
    const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null
    const root = ref.current
    if (!root) return

    const focusable = () => [...root.querySelectorAll<HTMLElement>(
      'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])',
    )].filter((el) => !el.hidden && el.getAttribute('aria-hidden') !== 'true')

    ;(focusable()[0] ?? root).focus()
    const keydown = (event: KeyboardEvent) => {
      if (event.key !== 'Tab') return
      const items = focusable()
      if (!items.length) {
        event.preventDefault()
        root.focus()
        return
      }
      const first = items[0]
      const last = items[items.length - 1]
      if (event.shiftKey && (document.activeElement === first || !root.contains(document.activeElement))) {
        event.preventDefault()
        last.focus()
      } else if (!event.shiftKey && (document.activeElement === last || !root.contains(document.activeElement))) {
        event.preventDefault()
        first.focus()
      }
    }
    root.addEventListener('keydown', keydown)
    return () => {
      root.removeEventListener('keydown', keydown)
      if (previous?.isConnected) previous.focus()
    }
  }, [active, ref])
}
