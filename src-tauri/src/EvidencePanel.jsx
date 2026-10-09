import { useEffect, useRef, useState } from "react";

export function EvidencePanel({ adapter, sessionId, pageId, pageUrl, onRun }) {
  const [observation, setObservation] = useState(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [selectedId, setSelectedId] = useState("");
  const [action, setAction] = useState("scroll_target");
  const [inputText, setInputText] = useState("");
  const [predicateMode, setPredicateMode] = useState("none");
  const [predicateValue, setPredicateValue] = useState("");
  const scope = `${sessionId}:${pageId}:${pageUrl}`;
  const scopeRef = useRef({ key: scope, generation: 0 });
  if (scopeRef.current.key !== scope) scopeRef.current = { key: scope, generation: scopeRef.current.generation + 1 };
  const refreshId = useRef(0);
  const mounted = useRef(true);
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; }; }, []);
  useEffect(() => {
    setObservation(null); setSelectedId(""); setInputText(""); setError(""); setBusy(false);
  }, [scope]);
  if (typeof adapter.getPageObservation !== "function") return null;

  const refresh = async () => {
    const requestedScope = scopeRef.current;
    const requestId = ++refreshId.current;
    const current = () => mounted.current && requestedScope === scopeRef.current && requestId === refreshId.current;
    setBusy(true); setError("");
    try {
      const next = await adapter.getPageObservation(sessionId, pageId);
      if (!current()) return;
      setObservation(next);
      setSelectedId(next.targets?.[0]?.id ?? "");
    } catch (error) {
      if (current()) {
        setObservation(null); setSelectedId(""); setError(String(error.message ?? error));
      }
    } finally {
      if (current()) setBusy(false);
    }
  };
  const propose = async (event) => {
    event.preventDefault();
    if (!observation?.document || !selectedId || busy) return;
    const requestedScope = scopeRef.current;
    const source = { sessionId, pageId, pageUrl: observation.url };
    let pendingRun = null;
    const args = { document: JSON.stringify(observation.document), target_id: selectedId };
    if (action === "type_target") args.text = inputText;
    if (predicateMode !== "none") {
      args.postcondition = JSON.stringify(predicateMode === "url_equals"
        ? { type: predicateMode, url: predicateValue } : { type: predicateMode, text: predicateValue });
    }
    setInputText(""); setBusy(true); setError("");
    try {
      const result = await adapter.executeBrowserTool(sessionId, pageId, { name: action, arguments: args });
      if (result.status === "awaiting_approval") pendingRun = result;
      if (requestedScope !== scopeRef.current && result.status === "awaiting_approval") {
        const cancelled = await adapter.cancelAgentRun(result.run_id);
        await onRun(cancelled, source);
        return;
      }
      // Every attempted dispatch retires this UI's references. A pending approval
      // retains its reviewed state in Rust and has its own approve/deny controls.
      if (mounted.current && requestedScope === scopeRef.current) {
        setObservation(null); setSelectedId("");
      }
      // Dispatch may already have happened: its receipt must survive a tab change.
      await onRun(result, source);
    } catch (error) {
      const message = String(error.message ?? error);
      if (mounted.current && requestedScope === scopeRef.current) {
        setObservation(null); setSelectedId(""); setError(message);
      }
      // A lost transport response may follow dispatch. Preserve the originating
      // page and any uncancelled grant instead of silently dropping this handoff.
      await onRun(pendingRun
        ? { ...pendingRun, final_response: `Previous-page approval is still pending; cancellation failed: ${message}` }
        : { status: "failed", final_response: `Action response unavailable; inspect ${source.pageUrl} before repeating: ${message}`, events: [] }, source);
    } finally {
      if (mounted.current && requestedScope === scopeRef.current) setBusy(false);
    }
  };
  const capabilities = observation?.capabilities;
  const target = observation?.targets?.find(target => target.id === selectedId);
  const canAct = observation?.document && capabilities?.interaction && target && !target.disabled;
  return <details className="evidence-panel">
    <summary>Page evidence and actions</summary>
    <button className="btn btn-secondary" type="button" onClick={refresh} disabled={busy || pageId == null}>
      {busy ? "Working…" : "Refresh evidence"}
    </button>
    {error && <p role="alert">{error}</p>}
    {observation && <>
      <p className="evidence-source">{observation.title || "Untitled page"}<br />{observation.url}</p>
      <p>{capabilities.javascript ? "JavaScript execution available" : "Static page extraction"} · {capabilities.scoped_targets ? "Scoped targets available" : "Scoped targets unavailable"}</p>
      {!capabilities.screenshots && <p className="evidence-limit">Visual evidence unavailable</p>}
      {!capabilities.enforcing_subresource_network && <p className="evidence-limit">Background network isolation unavailable</p>}
      <details><summary>Extracted page text</summary><pre className="evidence-text">{observation.text || "No text captured"}</pre></details>
      {observation.tables?.length > 0 && <details><summary>Tables ({observation.tables.length})</summary>
        {observation.tables.map((table, i) => <table className="evidence-table" key={i}>
          <thead><tr>{table.headers.map((cell, j) => <th key={j}>{cell}</th>)}</tr></thead>
          <tbody>{table.rows.map((row, j) => <tr key={j}>{row.map((cell, k) => <td key={k}>{cell}</td>)}</tr>)}</tbody>
        </table>)}
      </details>}
      {observation.omissions?.length > 0 && <details><summary>Evidence limits</summary>
        <ul>{observation.omissions.map((item, i) => <li key={i}>{item}</li>)}</ul>
      </details>}
      {observation.document && <p className="evidence-limit">Observation revision {observation.document.revision}. Refresh after page changes.</p>}
      {capabilities.scoped_targets && <form className="evidence-actions" onSubmit={propose}>
        <label>Observed target<select value={selectedId} onChange={event => setSelectedId(event.target.value)} disabled={busy}>
          <option value="">Select target</option>
          {observation.targets.map(target => <option value={target.id} key={target.id} disabled={target.disabled}>
            {target.role}: {target.label || target.tag}{target.disabled ? " (disabled)" : ""}
          </option>)}
        </select></label>
        {target?.destination && <p className="evidence-source">Destination: {target.destination}</p>}
        <label>Action<select value={action} onChange={event => setAction(event.target.value)} disabled={busy}>
          <option value="scroll_target">Scroll into view</option><option value="click_target">Click</option>
          <option value="type_target">Type</option><option value="submit_target">Submit form</option>
        </select></label>
        {action === "type_target" && <label>Input text<input value={inputText} type={target?.sensitive ? "password" : "text"} autoComplete="off" onChange={event => setInputText(event.target.value)} maxLength={16384} /></label>}
        <label>Evidence to check after dispatch<select value={predicateMode} onChange={event => setPredicateMode(event.target.value)} disabled={busy}>
          <option value="none">Report dispatch only</option><option value="text_contains">Page contains text</option><option value="url_equals">Page URL equals</option>
        </select></label>
        {predicateMode !== "none" && <label>Expected {predicateMode === "url_equals" ? "URL" : "text"}<input value={predicateValue} onChange={event => setPredicateValue(event.target.value)} required maxLength={4096} /></label>}
        <p className="evidence-limit">A matching page condition is evidence. Confirm important website outcomes yourself.</p>
        <button className="btn" disabled={!canAct || busy || (action === "type_target" && !inputText) || (predicateMode !== "none" && !predicateValue)} type="submit">Propose action</button>
      </form>}
    </>}
  </details>;
}
