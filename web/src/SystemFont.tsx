import { useState } from 'react'
import { helpFor } from './help'
import { api } from './api'
import { logAction, useStudio, type StudioNode } from './store'
import Tooltip from './Tooltip'
import messages from './messages'

/** Picks one of the computer's own fonts for a Text node. The built-in fonts stay available through the Font choice. */
export default function SystemFont({ node }: { node: StudioNode }) {
  const setParam = useStudio((s) => s.setParam)
  const toast = useStudio((s) => s.toast)
  const [fonts, setFonts] = useState<{ name: string; label: string }[] | null>(null)
  const [busy, setBusy] = useState(false)
  const current = String(node.data.params.system_font ?? '')
  if (!api.canListFonts()) return <p className="mut small">System fonts need a Chromium-based browser (Chrome, Edge, Brave).</p>

  const load = async () => {
    setBusy(true)
    try { const list = await api.systemFonts(); setFonts(list); logAction(`Listed ${list.length} system fonts`, node.id) } catch (e) { toast(String((e as Error).message ?? e), { level: 'error' }) }
    setBusy(false)
  }
  const pick = async (name: string) => {
    if (!name) return setParam(node.id, 'system_font', '')
    setBusy(true)
    try {
      await api.useFont(name)
      setParam(node.id, 'system_font', name)
      logAction(`Loaded system font: ${name}`, node.id)
    } catch (e) { toast(String((e as Error).message ?? e), { level: 'error' }) }
    setBusy(false)
  }
  const list = fonts ?? (current ? [{ name: current, label: current }] : [])
  return (
    <div className="row">
      <span>System font{current && <Tooltip content={messages.actions.backToBuiltInFont}><button className="linkish" onClick={() => void pick('')}>use built-in</button></Tooltip>}</span>
      {list.length > 0 && (
        <select aria-label="System font" value={current} disabled={busy} onChange={(e) => void pick(e.target.value)}>
          <option value="">built-in (see Font)</option>
          {list.map((f) => <option key={f.name} value={f.name}>{f.label}</option>)}
        </select>
      )}
      {!fonts && <Tooltip content={messages.actions.listSystemFonts}><button onClick={() => void load()} disabled={busy}>Use system fonts…</button></Tooltip>}
      <small className="help">{helpFor('text', 'system_font')}</small>
    </div>
  )
}
