import { useEffect, useState } from "react";
import { startPageSites } from "./historySuggestions.js";
import { omniboxActionHint } from "./omniboxInput.js";

/**
 * Local start page drawn by the shell. The page webview stays on about:blank
 * underneath until the user opens a site.
 */
export function NewTabPage({ adapter, onNavigate }) {
  const [query, setQuery] = useState("");
  const [sites, setSites] = useState([]);
  const hint = omniboxActionHint(query);

  useEffect(() => {
    if (typeof adapter?.searchBrowsingHistory !== "function") return undefined;
    let cancelled = false;
    Promise.resolve()
      .then(() => adapter.searchBrowsingHistory("", 8))
      .then((hits) => {
        if (!cancelled) setSites(startPageSites(hits));
      })
      .catch(() => {
        if (!cancelled) setSites([]);
      });
    return () => {
      cancelled = true;
    };
  }, [adapter]);

  return (
    <div className="start-page">
      <form
        className="start-search-form"
        onSubmit={(event) => {
          event.preventDefault();
          onNavigate(query);
        }}
      >
        <div className="start-search-field">
          <input
            aria-label="Search or enter a URL"
            autoComplete="off"
            className="start-search"
            onChange={(event) => setQuery(event.target.value)}
            placeholder="Search or enter a URL"
            spellCheck={false}
            value={query}
          />
          {hint ? <span className="start-search-hint">{hint}</span> : null}
        </div>
        <button className="btn" type="submit">
          Go
        </button>
      </form>
      {sites.length > 0 ? (
        <ul aria-label="Most visited" className="start-sites">
          {sites.map((site) => (
            <li key={site.url}>
              <button className="start-site" onClick={() => onNavigate(site.url)} type="button">
                <span className="start-site-title">{site.title}</span>
                <span className="start-site-host">{site.host}</span>
              </button>
            </li>
          ))}
        </ul>
      ) : (
        <p className="start-empty">Pages you open will show up here.</p>
      )}
    </div>
  );
}
