import { etaSeconds, formatEta, useNow } from "./eta";
import { useStudio } from "./store";

export default /** The one-line state of the engine: working (step, node and time left), draft, error (with a jump to the node that failed) or ready. */
function Status({ onGoto }: { onGoto: (id: string) => void }) {
  const busy = useStudio((s) => s.busy);
  const error = useStudio((s) => s.error);
  const draft = useStudio((s) => s.draft);
  const runs = useStudio((s) => s.runs);
  const nodes = useStudio((s) => s.nodes);
  const progress = useStudio((s) => s.progress);
  const now = useNow(busy);
  const titleOf = (id: string) => {
    const n = nodes.find((x) => x.id === id);
    return n?.data.title || useStudio.getState().types[n?.data.type ?? ""]?.label || id;
  };
  // the first node to fail is the cause; the ones after it only say an earlier node failed
  const failing = nodes.find((n) => runs[n.id]?.state === "error" && !runs[n.id]?.error?.startsWith("an earlier node failed"));
  const working = nodes.find((n) => runs[n.id]?.state === "running");
  const step = busy && progress.total > 0 ? ` step ${Math.min(Math.floor(progress.done) + 1, progress.total)} of ${progress.total}${working ? ` · ${titleOf(working.id)}` : ""}` : "";
  const left = busy ? etaSeconds(runs, now) : null;
  const text = error
    ? `⚠ ${error}`
    : failing && !busy
      ? `⚠ ${titleOf(failing.id)}: ${runs[failing.id]?.error}`
      : busy
      ? `${draft ? "refining" : "working"}…${step}${left !== null && left >= 3 ? ` ${formatEta(left)}` : ""}`
      : draft
        ? "draft preview"
        : "ready";
  return (
    <>
      <span
        className={`status${busy ? " busy" : ""}`}
        role="status"
        aria-live="polite"
        title={text}
      >
        {text}
      </span>
      {failing && !busy && (
        <button className="goto-fail" onClick={() => onGoto(failing.id)}>
          Go to {titleOf(failing.id)}
        </button>
      )}
    </>
  );
}
