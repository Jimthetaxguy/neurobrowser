import { useEffect, useId, useRef, useState } from "react";

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

function hostOf(pageUrl) {
  try {
    return new URL(pageUrl).hostname;
  } catch {
    return "";
  }
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
export function memoryQueryFromOmnibox(input) {
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

function formatScore(score) {
  const value = Number(score);
  if (!Number.isFinite(value)) return "";
  return value.toFixed(2);
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

export function OmniboxSuggestions({
  activeIndex,
  canExplain,
  canForget,
  explainingId,
  explanations,
  forgettingId,
  hits,
  listId,
  onExplain,
  onForget,
  onHighlight,
  onSelect,
  pendingForgetId,
  setPendingForgetId,
}) {
  const listRef = useRef(null);

  useEffect(() => {
    if (activeIndex < 0) return;
    const list = listRef.current;
    const option = list?.querySelector(`[data-index="${activeIndex}"]`);
    if (!list || !option) return;
    const listRect = list.getBoundingClientRect();
    const optionRect = option.getBoundingClientRect();
    if (optionRect.top < listRect.top) {
      list.scrollTop -= listRect.top - optionRect.top;
    } else if (optionRect.bottom > listRect.bottom) {
      list.scrollTop += optionRect.bottom - listRect.bottom;
    }
  }, [activeIndex, hits.length]);

  if (!hits.length) return null;

  return (
    <div className="omnibox-suggestions">
      <div className="omnibox-suggestions-label" id={`${listId}-label`}>
        From memory
      </div>
      <ul
        aria-labelledby={`${listId}-label`}
        className="omnibox-suggestions-list"
        id={listId}
        ref={listRef}
        role="listbox"
      >
        {hits.map((hit, index) => {
          const pageUrl = pageUrlOf(hit);
          const heading = headingLabel(hit);
          const excerpt = hitExcerpt(hit);
          const host = hostOf(pageUrl);
          const title = heading || excerpt || host || "Saved page";
          const detail = heading && excerpt ? excerpt : "";
          const selected = index === activeIndex;
          const explanation = explanations[hit.block_id];
          const confirmingForget = pendingForgetId === hit.block_id;
          const titleIsExcerpt = !heading && Boolean(excerpt);

          return (
            <li
              className={`omnibox-suggestion${selected ? " active" : ""}`}
              data-index={index}
              key={hit.block_id || `${pageUrl}-${index}`}
            >
              <div className="omnibox-suggestion-row">
                <button
                  aria-selected={selected}
                  className={`omnibox-suggestion-main${titleIsExcerpt ? " wrap-title" : ""}`}
                  id={optionDomId(listId, index)}
                  onClick={() => onSelect(hit)}
                  onMouseDown={(event) => event.preventDefault()}
                  onMouseEnter={() => onHighlight(index)}
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
        })}
      </ul>
    </div>
  );
}

export function Omnibox({ adapter, onNavigate, onStatus, setUrl, url }) {
  const listId = useId();
  const rootRef = useRef(null);
  // Keystrokes set this to the field value. Snapshot updates call setUrl and leave it unset.
  const userQueryRef = useRef(null);
  const requestId = useRef(0);
  const [hits, setHits] = useState([]);
  const [hitQuery, setHitQuery] = useState("");
  const [open, setOpen] = useState(false);
  const [activeIndex, setActiveIndex] = useState(-1);
  const [searching, setSearching] = useState(false);
  const [explanations, setExplanations] = useState({});
  const [explainingId, setExplainingId] = useState(null);
  const [pendingForgetId, setPendingForgetId] = useState(null);
  const [forgettingId, setForgettingId] = useState(null);

  const canExplain = typeof adapter?.explainMemoryResult === "function";
  const canForget = typeof adapter?.forgetMemory === "function";
  const visible = open && hits.length > 0;
  const safeIndex = activeIndex >= 0 && activeIndex < hits.length ? activeIndex : -1;

  useEffect(() => {
    if (userQueryRef.current !== url) {
      requestId.current += 1;
      setHits((current) => (current.length === 0 ? current : []));
      setOpen(false);
      setActiveIndex(-1);
      setHitQuery((current) => (current === "" ? current : ""));
      setSearching(false);
      setExplanations((current) => (Object.keys(current).length === 0 ? current : {}));
      setPendingForgetId(null);
      return undefined;
    }

    const query = memoryQueryFromOmnibox(url);
    if (!query || typeof adapter?.searchLocalMemory !== "function") {
      requestId.current += 1;
      setHits((current) => (current.length === 0 ? current : []));
      setOpen(false);
      setActiveIndex(-1);
      setHitQuery((current) => (current === "" ? current : ""));
      setSearching(false);
      return undefined;
    }

    const request = requestId.current + 1;
    requestId.current = request;
    setSearching(true);
    const timer = window.setTimeout(async () => {
      try {
        const next = await adapter.searchLocalMemory(query, SEARCH_LIMIT);
        if (requestId.current !== request) return;
        const list = Array.isArray(next) ? next : [];
        const focused = rootRef.current?.contains(document.activeElement) ?? false;
        setHits(list);
        setHitQuery(query);
        setExplanations({});
        setPendingForgetId(null);
        setActiveIndex(-1);
        setOpen(list.length > 0 && focused);
      } catch (error) {
        if (requestId.current !== request) return;
        setHits([]);
        setOpen(false);
        console.warn("memory search failed", messageText(error));
      } finally {
        if (requestId.current === request) setSearching(false);
      }
    }, SEARCH_DEBOUNCE_MS);

    return () => window.clearTimeout(timer);
  }, [adapter, url]);

  useEffect(() => {
    if (!visible) return undefined;
    const onPointerDown = (event) => {
      if (rootRef.current && !rootRef.current.contains(event.target)) {
        setOpen(false);
        setActiveIndex(-1);
      }
    };
    document.addEventListener("pointerdown", onPointerDown);
    return () => document.removeEventListener("pointerdown", onPointerDown);
  }, [visible]);

  function closeSuggestions() {
    userQueryRef.current = null;
    requestId.current += 1;
    setOpen(false);
    setHits([]);
    setActiveIndex(-1);
    setSearching(false);
  }

  function selectHit(hit) {
    const nextUrl = pageUrlOf(hit);
    if (!nextUrl) return;
    closeSuggestions();
    setUrl(nextUrl);
    onNavigate(nextUrl);
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
      setHits((current) => current.filter((item) => pageUrlOf(item) !== pageUrl));
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
      selectHit(hits[safeIndex]);
      return;
    }
    closeSuggestions();
    onNavigate();
  }

  function onKeyDown(event) {
    if (event.key === "Escape") {
      if (!visible) return;
      event.preventDefault();
      setOpen(false);
      setActiveIndex(-1);
      return;
    }
    if (!visible) return;
    if (event.key === "ArrowDown") {
      event.preventDefault();
      setActiveIndex((index) => (index + 1) % hits.length);
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      setActiveIndex((index) => (index <= 0 ? hits.length - 1 : index - 1));
    }
  }

  return (
    <form className="url-bar" onSubmit={onSubmit} ref={rootRef}>
      <div className="omnibox">
        <input
          aria-activedescendant={visible && safeIndex >= 0 ? optionDomId(listId, safeIndex) : undefined}
          aria-autocomplete="list"
          aria-busy={searching}
          aria-controls={visible ? listId : undefined}
          aria-expanded={visible}
          aria-label="URL or memory search"
          autoComplete="off"
          className="url-input"
          onChange={(event) => {
            const value = event.target.value;
            userQueryRef.current = value;
            setUrl(value);
          }}
          onFocus={() => {
            if (hits.length > 0 && userQueryRef.current === url) setOpen(true);
          }}
          onKeyDown={onKeyDown}
          placeholder="Enter a URL or domain"
          role="combobox"
          value={url}
        />
        {visible && (
          <OmniboxSuggestions
            activeIndex={safeIndex}
            canExplain={canExplain}
            canForget={canForget}
            explainingId={explainingId}
            explanations={explanations}
            forgettingId={forgettingId}
            hits={hits}
            listId={listId}
            onExplain={explainHit}
            onForget={forgetHit}
            onHighlight={setActiveIndex}
            onSelect={selectHit}
            pendingForgetId={pendingForgetId}
            setPendingForgetId={setPendingForgetId}
          />
        )}
      </div>
      <button className="btn" type="submit">
        Go
      </button>
    </form>
  );
}
