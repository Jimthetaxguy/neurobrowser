import { useEffect, useId, useRef, useState } from "react";
import { historyQueryFromInput, hostOf, suggestionRows } from "./historySuggestions.js";
import { omniboxActionHint } from "./omniboxInput.js";

const SEARCH_DEBOUNCE_MS = 200;
const SEARCH_LIMIT = 8;
const PLAIN_QUERY = /^[\p{L}\p{N}\s]+$/u;

function messageText(error) {
  if (error instanceof Error) return error.message;
  return String(error);
}

function plainText(value) {
  return String(value ?? "")
    .replace(/<[^>]*>/g, "")
    .replace(/\s+/g, " ")
    .trim();
}

function pageUrlOf(hit) {
  const value = hit?.page_url;
  if (typeof value === "string") return value;
  return "";
}

function headingLabel(hit) {
  if (!Array.isArray(hit?.heading_path)) return "";
  return hit.heading_path.map(plainText).filter(Boolean).join(" / ");
}

function hitExcerpt(hit) {
  const text = plainText(hit?.text);
  if (text.length <= 180) return text;
  return `${text.slice(0, 177)}…`;
}

/**
 * Terms to send to searchLocalMemory.
 * Letter-and-number queries pass through so Tantivy operators such as OR keep working.
 * Address-bar text with : / ? and other query syntax is reduced to words first.
 */
function memoryQueryFromOmnibox(input) {
  const trimmed = String(input ?? "").trim();
  if (!trimmed) return "";
  if (PLAIN_QUERY.test(trimmed)) return trimmed;
  return trimmed
    .split(/[^\p{L}\p{N}]+/u)
    .filter((token) => token.length > 1 && !/^(https?|www)$/i.test(token))
    .join(" ");
}

function optionDomId(listId, index) {
  return `${listId}-opt-${index}`;
}

function urlInputClass(hint) {
  if (hint === "Search DuckDuckGo") return "url-input hint-search";
  if (hint) return "url-input hint-go";
  return "url-input";
}

function formatScore(score) {
  const value = Number(score);
  if (!Number.isFinite(value)) return "";
  return value.toFixed(2);
}

function emptyList(current) {
  return current.length === 0 ? current : [];
}

function emptyMap(current) {
  return Object.keys(current).length === 0 ? current : {};
}

async function loadHistory(adapter, query, onStatus) {
  try {
    const next = await adapter.searchBrowsingHistory(query, SEARCH_LIMIT);
    return Array.isArray(next) ? next : [];
  } catch (error) {
    console.warn("history search failed", messageText(error));
    onStatus?.(`Could not search history: ${messageText(error)}`);
    return [];
  }
}

async function loadMemory(adapter, query) {
  try {
    const next = await adapter.searchLocalMemory(query, SEARCH_LIMIT);
    return Array.isArray(next) ? next : [];
  } catch (error) {
    console.warn("memory search failed", messageText(error));
    return [];
  }
}

function ExplainDetail({ detail }) {
  const snippets = (detail?.snippets ?? []).map(plainText).filter(Boolean);
  const breakdown = (detail?.breakdown ?? [])
    .map((part) => {
      const score = formatScore(part?.score);
      if (!part?.field || !score) return "";
      return `${part.field} ${score}`;
    })
    .filter(Boolean);

  if (!snippets.length && !breakdown.length) {
    return <div className="omnibox-explain">No extra match detail.</div>;
  }

  return (
    <div className="omnibox-explain">
      {snippets.map((snippet, index) => (
        <p key={`${index}-${snippet}`}>{snippet}</p>
      ))}
      {breakdown.length > 0 && <p className="omnibox-explain-score">{breakdown.join(" · ")}</p>}
    </div>
  );
}

function memoryPresentation(hit) {
  const pageUrl = pageUrlOf(hit);
  const heading = headingLabel(hit);
  const excerpt = hitExcerpt(hit);
  const host = hostOf(pageUrl);
  const title = heading || excerpt || host || "Saved page";
  return {
    detail: heading && excerpt ? excerpt : "",
    excerpt,
    heading,
    host,
    pageUrl,
    title,
    titleIsExcerpt: !heading && Boolean(excerpt),
  };
}

export function Omnibox({ adapter, onNavigate, onStatus, setUrl, url }) {
  const listId = useId();
  const rootRef = useRef(null);
  // Keystrokes set this to the field value. Snapshot updates call setUrl and leave it unset.
  const userQueryRef = useRef(null);
  const requestId = useRef(0);
  const [historyHits, setHistoryHits] = useState([]);
  const [memoryHits, setMemoryHits] = useState([]);
  const [hitQuery, setHitQuery] = useState("");
  const [focused, setFocused] = useState(false);
  const [dismissedQuery, setDismissedQuery] = useState(null);
  const [activeIndex, setActiveIndex] = useState(0);
  const [searching, setSearching] = useState(false);
  const [explanations, setExplanations] = useState({});
  const [explainingId, setExplainingId] = useState(null);
  const [pendingForgetId, setPendingForgetId] = useState(null);
  const [forgettingId, setForgettingId] = useState(null);

  const hintId = `${listId}-hint`;
  // Snapshot updates replace `url` without touching the draft, so the hint
  // follows an unsent edit and stays off the loaded page address.
  const editing = userQueryRef.current === url;
  const hint = editing ? omniboxActionHint(url) : "";
  const action = editing && String(url ?? "").trim() ? suggestionRows(url, [])[0] : null;
  const historyRows = editing && String(url ?? "").trim() ? suggestionRows("", historyHits) : [];
  const canExplain = typeof adapter?.explainMemoryResult === "function";
  const canForget = typeof adapter?.forgetMemory === "function";

  const entries = [];
  if (action) entries.push({ kind: "action", label: action.label, url: action.url });
  for (const row of historyRows) entries.push(row);
  for (const hit of memoryHits) {
    const presented = memoryPresentation(hit);
    entries.push({ kind: "memory", hit, url: presented.pageUrl, ...presented });
  }

  const visible = focused && dismissedQuery !== url && entries.length > 0;
  const safeIndex = entries.length === 0 ? -1 : Math.min(Math.max(activeIndex, 0), entries.length - 1);

  function commitInput(value) {
    userQueryRef.current = value;
    setUrl(value);
  }

  useEffect(() => {
    setActiveIndex(0);
  }, [url]);

  useEffect(() => {
    if (userQueryRef.current !== url || !String(url ?? "").trim()) {
      requestId.current += 1;
      setHistoryHits(emptyList);
      setMemoryHits(emptyList);
      setHitQuery((current) => (current === "" ? current : ""));
      setSearching(false);
      setExplanations(emptyMap);
      setPendingForgetId(null);
      return undefined;
    }

    const historyQuery = historyQueryFromInput(url);
    const memoryQuery = memoryQueryFromOmnibox(url);
    const canHistory = typeof adapter?.searchBrowsingHistory === "function";
    const canMemory = Boolean(memoryQuery) && typeof adapter?.searchLocalMemory === "function";
    if (!canHistory && !canMemory) {
      requestId.current += 1;
      setHistoryHits(emptyList);
      setMemoryHits(emptyList);
      setSearching(false);
      return undefined;
    }

    const request = requestId.current + 1;
    requestId.current = request;
    setSearching(true);
    setHistoryHits(emptyList);
    setMemoryHits(emptyList);
    setExplanations(emptyMap);
    setPendingForgetId(null);
    const timer = window.setTimeout(async () => {
      const [nextHistory, nextMemory] = await Promise.all([
        canHistory ? loadHistory(adapter, historyQuery, onStatus) : Promise.resolve([]),
        canMemory ? loadMemory(adapter, memoryQuery) : Promise.resolve([]),
      ]);
      if (requestId.current !== request) return;
      setHistoryHits(nextHistory);
      setMemoryHits(nextMemory);
      setHitQuery(canMemory ? memoryQuery : "");
      setSearching(false);
    }, SEARCH_DEBOUNCE_MS);

    return () => window.clearTimeout(timer);
  }, [adapter, onStatus, url]);

  useEffect(() => {
    if (safeIndex < 0) return;
    const list = rootRef.current?.querySelector(".omnibox-suggestions");
    const option = list?.querySelector(`[data-index="${safeIndex}"]`);
    if (!list || !option) return;
    const listRect = list.getBoundingClientRect();
    const optionRect = option.getBoundingClientRect();
    if (optionRect.top < listRect.top) {
      list.scrollTop -= listRect.top - optionRect.top;
    } else if (optionRect.bottom > listRect.bottom) {
      list.scrollTop += optionRect.bottom - listRect.bottom;
    }
  }, [safeIndex, entries.length]);

  useEffect(() => {
    if (!visible) return undefined;
    const onPointerDown = (event) => {
      if (rootRef.current && !rootRef.current.contains(event.target)) {
        setDismissedQuery(url);
      }
    };
    document.addEventListener("pointerdown", onPointerDown);
    return () => document.removeEventListener("pointerdown", onPointerDown);
  }, [url, visible]);

  function closeSuggestions() {
    userQueryRef.current = null;
    requestId.current += 1;
    setDismissedQuery(null);
    setHistoryHits([]);
    setMemoryHits([]);
    setHitQuery("");
    setActiveIndex(0);
    setSearching(false);
    setExplanations({});
    setPendingForgetId(null);
  }

  function selectEntry(entry) {
    if (entry.kind === "memory" && !entry.url) return;
    closeSuggestions();
    if (entry.kind === "action") onNavigate();
    else onNavigate(entry.url);
  }

  async function explainHit(hit) {
    if (!canExplain) return;
    if (explanations[hit.block_id]) {
      setExplanations((current) => {
        const next = { ...current };
        delete next[hit.block_id];
        return next;
      });
      return;
    }
    if (!hitQuery) return;
    setExplainingId(hit.block_id);
    try {
      const detail = await adapter.explainMemoryResult(hitQuery, hit, SEARCH_LIMIT);
      setExplanations((current) => ({ ...current, [hit.block_id]: detail }));
    } catch (error) {
      onStatus?.(`Could not explain that memory hit: ${messageText(error)}`);
    } finally {
      setExplainingId(null);
    }
  }

  async function forgetHit(hit) {
    const pageUrl = pageUrlOf(hit);
    if (!pageUrl || !canForget) return;
    setForgettingId(hit.block_id);
    try {
      await adapter.forgetMemory(pageUrl);
      setMemoryHits((current) => current.filter((item) => pageUrlOf(item) !== pageUrl));
      setExplanations((current) => {
        const next = { ...current };
        delete next[hit.block_id];
        return next;
      });
      setPendingForgetId(null);
      const host = hostOf(pageUrl);
      onStatus?.(
        host
          ? `Forgot ${host} and blocked further capture for that site.`
          : "Forgot that page and removed it from memory."
      );
    } catch (error) {
      onStatus?.(`Could not forget that page: ${messageText(error)}`);
    } finally {
      setForgettingId(null);
    }
  }

  function onSubmit(event) {
    event.preventDefault();
    if (visible && safeIndex >= 0) {
      selectEntry(entries[safeIndex]);
      return;
    }
    closeSuggestions();
    onNavigate();
  }

  function onKeyDown(event) {
    if (event.key === "Escape") {
      if (!visible) return;
      event.preventDefault();
      setDismissedQuery(url);
      setActiveIndex(0);
      return;
    }
    if (event.altKey || event.metaKey || event.ctrlKey) return;
    if (!visible || entries.length === 0) return;
    if (event.key === "ArrowDown") {
      event.preventDefault();
      setActiveIndex((safeIndex + 1) % entries.length);
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      setActiveIndex((safeIndex - 1 + entries.length) % entries.length);
    }
  }

  function onInputBlur(event) {
    const next = event.relatedTarget;
    if (next && rootRef.current?.contains(next)) return;
    window.queueMicrotask(() => {
      if (!rootRef.current?.contains(document.activeElement)) setFocused(false);
    });
  }

  let cursor = 0;
  const actionEntry = action ? { ...entries[cursor], index: cursor++ } : null;
  const historyEntries = historyRows.map((row) => ({ ...row, index: cursor++ }));
  const memoryEntries = entries.slice(cursor).map((row) => ({ ...row, index: cursor++ }));

  return (
    <form className="url-bar" onSubmit={onSubmit} ref={rootRef}>
      <div className="omnibox">
        <input
          aria-activedescendant={visible && safeIndex >= 0 ? optionDomId(listId, safeIndex) : undefined}
          aria-autocomplete="list"
          aria-busy={searching}
          aria-controls={visible ? listId : undefined}
          aria-describedby={hint ? hintId : undefined}
          aria-expanded={visible}
          aria-label="URL or search"
          autoComplete="off"
          className={urlInputClass(hint)}
          onBlur={onInputBlur}
          onChange={(event) => commitInput(event.target.value)}
          onFocus={() => setFocused(true)}
          onKeyDown={onKeyDown}
          onKeyUp={(event) => commitInput(event.currentTarget.value)}
          placeholder="Enter a URL or domain"
          role="combobox"
          spellCheck={false}
          value={url}
        />
        {hint ? (
          <span className="omnibox-hint" id={hintId}>
            {hint}
          </span>
        ) : null}
        {visible && (
          <div className="omnibox-suggestions">
            <ul
              aria-label="Address suggestions"
              className="omnibox-suggestions-list"
              id={listId}
              role="listbox"
            >
              {actionEntry && (
                <SuggestionRow
                  entry={actionEntry}
                  listId={listId}
                  onHighlight={setActiveIndex}
                  onSelect={selectEntry}
                  selected={actionEntry.index === safeIndex}
                />
              )}
              {historyEntries.length > 0 && (
                <li className="omnibox-suggestions-label" role="presentation">
                  History
                </li>
              )}
              {historyEntries.map((entry) => (
                <SuggestionRow
                  entry={entry}
                  key={`history-${entry.url}-${entry.index}`}
                  listId={listId}
                  onHighlight={setActiveIndex}
                  onSelect={selectEntry}
                  selected={entry.index === safeIndex}
                />
              ))}
              {memoryEntries.length > 0 && (
                <li className="omnibox-suggestions-label" role="presentation">
                  Memory
                </li>
              )}
              {memoryEntries.map((entry) => (
                <MemorySuggestion
                  canExplain={canExplain}
                  canForget={canForget}
                  entry={entry}
                  explainingId={explainingId}
                  explanation={explanations[entry.hit.block_id]}
                  forgettingId={forgettingId}
                  key={entry.hit.block_id || `memory-${entry.index}`}
                  listId={listId}
                  onExplain={explainHit}
                  onForget={forgetHit}
                  onHighlight={setActiveIndex}
                  onSelect={selectEntry}
                  pendingForgetId={pendingForgetId}
                  selected={entry.index === safeIndex}
                  setPendingForgetId={setPendingForgetId}
                />
              ))}
            </ul>
          </div>
        )}
      </div>
      <button className="btn" type="submit">
        Go
      </button>
    </form>
  );
}

function SuggestionRow({ entry, listId, onHighlight, onSelect, selected }) {
  return (
    <li
      className={`omnibox-suggestion${selected ? " active" : ""}`}
      data-index={entry.index}
      data-kind={entry.kind}
      data-url={entry.url}
    >
      <button
        aria-selected={selected}
        className="omnibox-suggestion-main"
        id={optionDomId(listId, entry.index)}
        onClick={() => onSelect(entry)}
        onMouseDown={(event) => event.preventDefault()}
        onMouseEnter={() => onHighlight(entry.index)}
        role="option"
        type="button"
      >
        <span className="omnibox-suggestion-title">{entry.label}</span>
        {entry.kind === "history" && entry.detail ? (
          <span className="omnibox-suggestion-url">{entry.detail}</span>
        ) : null}
      </button>
    </li>
  );
}

function MemorySuggestion({
  canExplain,
  canForget,
  entry,
  explainingId,
  explanation,
  forgettingId,
  listId,
  onExplain,
  onForget,
  onHighlight,
  onSelect,
  pendingForgetId,
  selected,
  setPendingForgetId,
}) {
  const { hit, host, pageUrl, title, detail, titleIsExcerpt } = entry;
  const confirmingForget = pendingForgetId === hit.block_id;

  return (
    <li
      className={`omnibox-suggestion${selected ? " active" : ""}`}
      data-index={entry.index}
      data-kind="memory"
      data-url={pageUrl}
    >
      <div className="omnibox-suggestion-row">
        <button
          aria-selected={selected}
          className={`omnibox-suggestion-main${titleIsExcerpt ? " wrap-title" : ""}`}
          id={optionDomId(listId, entry.index)}
          onClick={() => onSelect(entry)}
          onMouseDown={(event) => event.preventDefault()}
          onMouseEnter={() => onHighlight(entry.index)}
          role="option"
          type="button"
        >
          <span className="omnibox-suggestion-title">{title}</span>
          {pageUrl && <span className="omnibox-suggestion-url">{pageUrl}</span>}
          {detail && <span className="omnibox-suggestion-text">{detail}</span>}
        </button>
        {(canExplain || canForget) && (
          <div className="omnibox-suggestion-actions">
            {canExplain && (
              <button
                className="omnibox-suggestion-action"
                disabled={explainingId === hit.block_id}
                onClick={() => onExplain(hit)}
                onMouseDown={(event) => event.preventDefault()}
                type="button"
              >
                {explainingId === hit.block_id ? "…" : explanation ? "Hide" : "Why"}
              </button>
            )}
            {canForget && confirmingForget && (
              <>
                <span className="omnibox-forget-prompt">
                  {host ? `Stop capturing ${host}?` : "Forget this page?"}
                </span>
                <button
                  className="omnibox-suggestion-action danger"
                  disabled={forgettingId === hit.block_id}
                  onClick={() => onForget(hit)}
                  onMouseDown={(event) => event.preventDefault()}
                  type="button"
                >
                  Forget
                </button>
                <button
                  className="omnibox-suggestion-action"
                  onClick={() => setPendingForgetId(null)}
                  onMouseDown={(event) => event.preventDefault()}
                  type="button"
                >
                  Cancel
                </button>
              </>
            )}
            {canForget && !confirmingForget && (
              <button
                className="omnibox-suggestion-action"
                onClick={() => setPendingForgetId(hit.block_id)}
                onMouseDown={(event) => event.preventDefault()}
                type="button"
              >
                Forget
              </button>
            )}
          </div>
        )}
      </div>
      {explanation && <ExplainDetail detail={explanation} />}
    </li>
  );
}
