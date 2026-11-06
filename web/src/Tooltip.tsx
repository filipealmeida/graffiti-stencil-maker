import { cloneElement, useCallback, useId, useRef, useState, type FocusEvent, type MouseEvent, type ReactElement } from 'react'

/** A consistent hover/focus tooltip with an accessible description on its trigger. */
export default function Tooltip({ content, children }: { content: string; children: ReactElement }) {
  const id = useId()
  const wrap = useRef<HTMLSpanElement>(null)
  const [position, setPosition] = useState<{ left: number; top: number }>()
  const existing = children.props['aria-describedby']
  const positionTooltip = useCallback(() => {
    const trigger = wrap.current?.firstElementChild
    const tooltip = wrap.current?.querySelector<HTMLElement>('.tooltip-content')
    if (!trigger || !tooltip) return
    const anchor = trigger.getBoundingClientRect()
    const bounds = tooltip.getBoundingClientRect()
    const margin = 8
    const left = Math.max(margin, Math.min(
      anchor.left + (anchor.width - bounds.width) / 2,
      window.innerWidth - bounds.width - margin,
    ))
    const above = anchor.top - bounds.height - 6
    const top = above >= margin ? above : anchor.bottom + 6
    setPosition({ left, top })
  }, [])
  const trigger = cloneElement(children, {
    'aria-describedby': [existing, id].filter(Boolean).join(' '),
    'aria-description': children.props['aria-description'] ?? content,
    onMouseEnter: (event: MouseEvent) => {
      children.props.onMouseEnter?.(event)
      positionTooltip()
    },
    onFocus: (event: FocusEvent) => {
      children.props.onFocus?.(event)
      positionTooltip()
    },
  })
  return (
    <span className="tooltip-wrap" ref={wrap}>
      {trigger}
      <span className="tooltip-content" id={id} role="tooltip" style={{ left: position?.left ?? 8, top: position?.top ?? 0 }}>{content}</span>
    </span>
  )
}
