import type { NodeType, ParamDef } from './api'
import { useStudio, type StudioNode } from './store'

function Field({ p, value, onChange }: { p: ParamDef; value: unknown; onChange: (v: unknown) => void }) {
  if (p.kind === 'bool')
    return <label className="row chk"><input type="checkbox" checked={!!value} onChange={(e) => onChange(e.target.checked)} />{p.label}</label>
  if (p.kind === 'choice')
    return <label className="row"><span>{p.label}</span><select value={String(value)} onChange={(e) => onChange(e.target.value)}>{p.choices!.map((c) => <option key={c}>{c}</option>)}</select></label>
  if (p.kind === 'text') return null
  return (
    <label className="row">
      <span>{p.label} <em>{String(value)}</em></span>
      <input type="range" min={p.min} max={p.max} step={p.step} value={Number(value)} onChange={(e) => onChange(+e.target.value)} />
    </label>
  )
}

export default function ParamForm({ node, type }: { node: StudioNode; type: NodeType }) {
  const setParam = useStudio((s) => s.setParam)
  const groups = [...new Set(type.params.map((p) => p.group ?? ''))]
  return (
    <>
      {groups.map((g) => (
        <fieldset key={g}>
          {g && <legend>{g}</legend>}
          {type.params.filter((p) => (p.group ?? '') === g).map((p) => (
            <Field key={p.name} p={p} value={node.data.params[p.name]} onChange={(v) => setParam(node.id, p.name, v)} />
          ))}
        </fieldset>
      ))}
    </>
  )
}
