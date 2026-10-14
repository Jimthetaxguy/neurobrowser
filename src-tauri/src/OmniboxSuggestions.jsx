import { useEffect, useId, useRef, useState } from "react";
import { historyQueryFromInput, suggestionRows } from "./historySuggestions.js";
import { omniboxActionHint } from "./omniboxInput.js";

const SEARCH_DEBOUNCE_MS = 200;

function messageText(error) {
  if (error instanceof Error) return error.message;
  return String(error);
}

function optionDomId(listId, index) {
  return `${listId}-opt-${index}`;
}

function urlInputClass(hint) {
  if (hint === "Search DuckDuckGo") return "url-input hint-search";
  if (hint) return "url-input hint-go";
  return "url-input";
}

export function Omnibox({ adapter, onNavigate, onStatus, setUrl, url }) {
  const listId = useId();
  const rootRef = useRef(null);
  // Keystrokes set this to the field value. Snapshot updates call setUrl and leave it unset.
  const userQueryRef = useRef(null);
  const requestId = useRef(0);
  const [hits, setHits] = useState([]);
  const [focused, setFocused] = useState(false);
  const [dismissedQuery, setDismissedQuery] = useState(null);
  const [activeIndex, setActiveIndex] = useState(0);
  const [searching, setSearching] = useState(false);

  const hintId = `${listId}-hint`;
  // Snapshot updates replace `url` without touching the draft, so the hint
  // follows an unsent edit and stays off the loaded page address.
  const editing = userQueryRef.current === url;
  const hint = editing ? omniboxActionHint(url) : "";
  const rows = editing && String(url ?? "").trim() ? suggestionRows(url, hits) : [];
  const visible = focused && dismissedQuery !== url && rows.length > 0;
  const safeIndex = rows.length === 0 ? -1 : Math.min(Math.max(activeIndex, 0), rows.length - 1);

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
      setHits((current) => (current.length === 0 ? current : []));
      setSearching(false);
      return undefined;
    }
    if (typeof adapter?.searchBrowsingHistory !== "function") {
      requestId.current += 1;
      setHits((current) => (current.length === 0 ? current : []));
      setSearching(false);
      return undefined;
    }

    const query = historyQueryFromInput(url);
    const request = requestId.current + 1;
    requestId.current = request;
    setSearching(true);
    setHits((current) => (current.length === 0 ? current : []));
    const timer = window.setTimeout(async () => {
      try {
        const next = await adapter.searchBrowsingHistory(query, 8);
        if (requestId.current !== request) return;
        setHits(Array.isArray(next) ? next : []);
      } catch (error) {
        if (requestId.current !== request) return;
        setHits([]);
        console.warn("history search failed", messageText(error));
        onStatus?.(`Could not search history: ${messageText(error)}`);
      } finally {
        if (requestId.current === request) setSearching(false);
      }
    }, SEARCH_DEBOUNCE_MS);

    return () => window.clearTimeout(timer);
  }, [adapter, onStatus, url]);

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
    setHits([]);
    setActiveIndex(0);
    setSearching(false);
  }

  function selectRow(row) {
    closeSuggestions();
    if (row.kind === "history") onNavigate(row.url);
    else onNavigate();
  }

  function onSubmit(event) {
    event.preventDefault();
    if (visible && safeIndex >= 0) {
      selectRow(rows[safeIndex]);
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
    if (!visible) return;
    if (event.key === "ArrowDown") {
      event.preventDefault();
      setActiveIndex((safeIndex + 1) % rows.length);
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      setActiveIndex((safeIndex - 1 + rows.length) % rows.length);
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
          aria-describedby={hint ? hintId : undefined}
          aria-expanded={visible}
          aria-label="URL or search"
          autoComplete="off"
          className={urlInputClass(hint)}
          onBlur={() => setFocused(false)}
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
              {rows.map((row, index) => {
                const selected = index === safeIndex;
                return (
                  <li
                    className={`omnibox-suggestion${selected ? " active" : ""}`}
                    data-index={index}
                    data-kind={row.kind}
                    data-url={row.url}
                    key={`${row.kind}-${row.url}-${index}`}
                  >
                    <button
                      aria-selected={selected}
                      className="omnibox-suggestion-main"
                      id={optionDomId(listId, index)}
                      onClick={() => selectRow(row)}
                      onMouseDown={(event) => event.preventDefault()}
                      onMouseEnter={() => setActiveIndex(index)}
                      role="option"
                      type="button"
                    >
                      <span className="omnibox-suggestion-title">{row.label}</span>
                      {row.kind === "history" && row.detail ? (
                        <span className="omnibox-suggestion-url">{row.detail}</span>
                      ) : null}
                    </button>
                  </li>
                );
              })}
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
