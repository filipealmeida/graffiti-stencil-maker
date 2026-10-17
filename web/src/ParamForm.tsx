import { useEffect, useId, useState } from 'react'
import type { NodeType, ParamDef } from './api'
import { useStudio, type StudioNode } from './store'
import { PARAM_HELP, unitOf } from './help'

const same = (a: unknown, b: unknown) => a === b || (typeof a === 'number' && typeof b === 'number' && Math.abs(a - b) < 1e-9)

/** A number box beside the slider: typing commits values in range at once, and the value is clamped when the box loses focus. */
function NumberBox({ p, value, onChange }: { p: ParamDef; value: number; onChange: (v: number) => void }) {
  const [draft, setDraft] = useState(String(value))
  useEffect(() => { if (parseFloat(draft) !== value) setDraft(String(value)) }, [value])
  const inRange = (v: number) => Number.isFinite(v) && v >= (p.min ?? -Infinity) && v <= (p.max ?? Infinity)
  return (
    <input
      className="numbox" type="number" min={p.min} max={p.max} step={p.step} value={draft} aria-label={p.label}
      onChange={(e) => { setDraft(e.target.value); const v = parseFloat(e.target.value); if (inRange(v)) onChange(v) }}
      onBlur={() => {
        const v = parseFloat(draft)
        const c = Number.isFinite(v) ? Math.min(p.max ?? v, Math.max(p.min ?? v, v)) : value
        setDraft(String(c))
        if (c !== value) onChange(c)
      }}
    />
  )
}

function Reset({ p, value, onChange }: { p: ParamDef; value: unknown; onChange: (v: unknown) => void }) {
  if (same(value, p.default)) return null
  const text = `Reset ${p.label} to ${String(p.default)}`
  return <button className="reset" title={`Reset to ${String(p.default)}`} aria-label={text} onClick={(e) => { e.preventDefault(); onChange(p.default) }}>↺</button>
}

/** The one-line reason behind a setting: a tooltip on the whole row and a line that shows on hover or focus. */
function Help({ p, id }: { p: ParamDef; id: string }) {
  const text = PARAM_HELP[p.name]
  return text ? <small className="help" id={id}>{text}</small> : null
}

function Field({ p, value, onChange }: { p: ParamDef; value: unknown; onChange: (v: unknown) => void }) {
  const hid = `help-${useId()}`
  const help = PARAM_HELP[p.name]
  const tip = help ? { title: help } : {}
  const desc = help ? { 'aria-describedby': hid } : {}
  if (p.kind === 'bool')
    return <label className="row chk" {...tip}><input type="checkbox" checked={!!value} onChange={(e) => onChange(e.target.checked)} {...desc} />{p.label}<Reset p={p} value={value} onChange={onChange} /><Help p={p} id={hid} /></label>
  if (p.kind === 'choice')
    return <label className="row" {...tip}><span>{p.label}<Reset p={p} value={value} onChange={onChange} /></span><select value={String(value)} onChange={(e) => onChange(e.target.value)} {...desc}>{p.choices!.map((c) => <option key={c}>{c}</option>)}</select><Help p={p} id={hid} /></label>
  if (p.kind === 'string')
    return <label className="row" {...tip}><span>{p.label}<Reset p={p} value={value} onChange={onChange} /></span><textarea rows={2} value={String(value)} onChange={(e) => onChange(e.target.value)} {...desc} /><Help p={p} id={hid} /></label>
  if (p.kind === 'text') return null
  const unit = unitOf(p.label)
  return (
    <label className="row" {...tip}>
      <span>{p.label} <em>{String(value)}{unit ? ` ${unit}` : ''}</em><Reset p={p} value={value} onChange={onChange} /></span>
      <span className="sliderrow">
        <input type="range" min={p.min} max={p.max} step={p.step} value={Number(value)} onChange={(e) => onChange(+e.target.value)} aria-valuetext={`${value}${unit ? ` ${unit}` : ''}`} {...desc} />
        <NumberBox p={p} value={Number(value)} onChange={onChange} />
        {unit && <span className="unit" aria-hidden="true">{unit}</span>}
      </span>
      <Help p={p} id={hid} />
    </label>
  )
}

export default function ParamForm({ node, type, layers }: { node: StudioNode; type: NodeType; layers?: number }) {
  const setParam = useStudio((s) => s.setParam)
  const groups = [...new Set(type.params.map((p) => p.group ?? ''))]
  // the picture of an Image node is not a setting
  const editable = type.params.filter((p) => p.kind !== 'text')
  const changed = editable.filter((p) => !same(node.data.params[p.name] ?? p.default, p.default))
  return (
    <>
      {groups.map((g) => (
        <fieldset key={g}>
          {g && <legend>{g}</legend>}
          {type.params.filter((p) => (p.group ?? '') === g).map((p) => {
            const v = node.data.params[p.name] ?? p.default
            const isLayer = layers && node.data.type === 'complexity' && (p.name === 'layer_from' || p.name === 'layer_to')
            if (!isLayer) return <Field key={p.name} p={p} value={v} onChange={(x) => setParam(node.id, p.name, x)} />
            // layer_to = 0 means "the top one", shown as the real maximum of the stencil above
            const shown = p.name === 'layer_to' && Number(v) === 0 ? layers : Number(v)
            return (
              <Field key={p.name} p={{ ...p, min: 1, max: layers, default: p.name === 'layer_to' ? layers : p.default }} value={Math.min(shown, layers)}
                onChange={(x) => setParam(node.id, p.name, p.name === 'layer_to' && x === layers ? 0 : x)} />
            )
          })}
        </fieldset>
      ))}
      {changed.length > 0 && (
        <button className="reset-all" onClick={() => changed.forEach((p) => setParam(node.id, p.name, p.default))}>
          Reset {changed.length} changed setting{changed.length > 1 ? 's' : ''} to default
        </button>
      )}
    </>
  )
}
