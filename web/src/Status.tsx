import { etaSeconds, formatEta, useNow } from "./eta";
import { useStudio } from "./store";

export default /** The one-line state of the engine: working (with the time left when it is long), draft, error or ready. */
function Status() {
  const busy = useStudio((s) => s.busy);
  const error = useStudio((s) => s.error);
  const draft = useStudio((s) => s.draft);
  const runs = useStudio((s) => s.runs);
  const now = useNow(busy);
  const left = busy ? etaSeconds(runs, now) : null;
  const text = error
    ? `⚠ ${error}`
    : busy
      ? `${draft ? "refining" : "working"}…${left !== null && left >= 3 ? ` ${formatEta(left)}` : ""}`
      : draft
        ? "draft preview"
        : "ready";
  return (
    <span
      className={`status${busy ? " busy" : ""}`}
      role="status"
      aria-live="polite"
    >
      {text}
    </span>
  );
}
