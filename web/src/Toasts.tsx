import { useStudio } from './store'

/** Short messages in the corner: what just happened, with Undo where the action can be taken back. */
export default function Toasts() {
  const toasts = useStudio((s) => s.toasts)
  const dismiss = useStudio((s) => s.dismissToast)
  return (
    <div className="toasts" role="status" aria-live="polite">
      {toasts.map((t) => (
        <div key={t.id} className={`toast ${t.level}`}>
          <span>{t.text}</span>
          {t.undo && <button onClick={() => { t.undo?.(); dismiss(t.id) }}>Undo</button>}
          <button className="x" aria-label="Dismiss" onClick={() => dismiss(t.id)}>✕</button>
        </div>
      ))}
    </div>
  )
}
