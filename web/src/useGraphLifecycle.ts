import { useEffect, useRef, type RefObject } from 'react'
import { runGraph, useStudio } from './store'

/** Debounces graph evaluation and keeps the fast first run / slider draft behavior. */
export default function useGraphLifecycle(
  signature: string,
  ready: boolean,
  sliding: RefObject<boolean>,
) {
  const first = useRef(true)
  const types = useStudio((state) => state.types)

  useEffect(() => {
    if (!ready || !Object.keys(types).length) return
    const wait = first.current ? 0 : sliding.current ? 150 : 350
    const timer = window.setTimeout(() => runGraph(sliding.current ?? false), wait)
    first.current = false
    return () => window.clearTimeout(timer)
  }, [ready, signature, sliding, types])
}
