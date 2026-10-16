import { Component, type ReactNode } from 'react'
import { STORE_KEY } from './store'

/** Last line of defence: a render error shows this page instead of a black window, and the autosaved graph stays recoverable. */
export default class ErrorBoundary extends Component<{ children: ReactNode }, { error: Error | null }> {
  state = { error: null as Error | null }
  static getDerivedStateFromError(error: Error) { return { error } }
  componentDidCatch(error: Error) { console.error('studio crashed:', error) }

  download = () => {
    const url = URL.createObjectURL(new Blob([localStorage.getItem(STORE_KEY) ?? '{}'], { type: 'application/json' }))
    const a = document.createElement('a')
    a.href = url
    a.download = 'autosave.stencil-graph.json'
    a.click()
    setTimeout(() => URL.revokeObjectURL(url), 10_000)
  }

  render() {
    if (!this.state.error) return this.props.children
    return (
      <div className="crash" role="alert">
        <h2>Something went wrong</h2>
        <p>The studio hit an unexpected error. Your graph is autosaved in this browser, so reloading usually brings it back.</p>
        <pre>{this.state.error.message}</pre>
        <div className="crash-actions">
          <button onClick={() => location.reload()}>Reload</button>
          <button onClick={this.download}>Download the autosaved graph</button>
          <button onClick={() => { if (window.confirm('Delete the autosaved graph and start empty?')) { localStorage.removeItem(STORE_KEY); location.reload() } }}>Discard autosave and reload</button>
        </div>
      </div>
    )
  }
}
