import { classifyOmniboxInput } from "./omniboxInput.js";

/** Address-bar and new-tab history rows. The command clamps to this as well. */
export const HISTORY_SUGGESTION_LIMIT = 8;

/** A tab whose page webview is still the empty document. */
export function isBlankDocument(url) {
  const value = String(url ?? "").trim();
  return value === "" || /^about:blank$/i.test(value);
}

/**
 * Text to match against history URLs and titles.
 * A leading "?" is the force-search operator, not part of the match.
 */
export function historyQueryFromInput(input) {
  const trimmed = String(input ?? "").trim();
  if (!trimmed) return "";
  if (trimmed.startsWith("?")) return trimmed.slice(1).trim();
  return trimmed;
}

export function hostOf(url) {
  try {
    return new URL(url).hostname;
  } catch {
    return "";
  }
}

/** "Search DuckDuckGo for …", "Go to …", or "" when the text submits nothing. */
export function actionLabel(input) {
  const destination = classifyOmniboxInput(input);
  if (!destination) return "";
  if (destination.kind === "search") return `Search DuckDuckGo for ${destination.query}`;
  return `Go to ${destination.url}`;
}

/**
 * Dropdown rows. The typed action is first. History follows in the order the
 * store query returned, capped at {@link HISTORY_SUGGESTION_LIMIT}.
 */
export function suggestionRows(input, hits) {
  const rows = [];
  const label = actionLabel(input);
  const destination = classifyOmniboxInput(input);
  if (label && destination) {
    rows.push({ kind: "action", label, url: destination.url });
  }
  const list = Array.isArray(hits) ? hits : [];
  for (const hit of usableHits(list).slice(0, HISTORY_SUGGESTION_LIMIT)) {
    const title = typeof hit.title === "string" ? hit.title.trim() : "";
    const host = hostOf(hit.url);
    rows.push({
      kind: "history",
      label: title || host || hit.url,
      detail: host || hit.url,
      url: hit.url,
    });
  }
  return rows;
}

/** New-tab sites: title and host, in the order history returned them. */
export function startPageSites(hits) {
  const list = Array.isArray(hits) ? hits : [];
  return usableHits(list)
    .slice(0, HISTORY_SUGGESTION_LIMIT)
    .map((hit) => {
      const title = typeof hit.title === "string" ? hit.title.trim() : "";
      const host = hostOf(hit.url) || hit.url;
      return { url: hit.url, title: title || host, host };
    });
}

/** Drop rows with no URL before the cap, so a blank entry does not take a slot. */
function usableHits(hits) {
  const usable = [];
  for (const hit of hits) {
    const url = typeof hit?.url === "string" ? hit.url.trim() : "";
    if (!url) continue;
    usable.push({ ...hit, url });
  }
  return usable;
}
