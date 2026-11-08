import type { NodeType } from './api'

export default function CatalogueLabel({ type, title }: { type: Pick<NodeType, 'label' | 'experimental'>; title?: string }) {
  return <>{title || type.label}{type.experimental && <small className="tag experimental-tag">Experimental</small>}</>
}
