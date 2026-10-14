// AppKit status bookkeeping, keyed by page and WebKit navigation id.
//
// The native host numbers every main-frame navigation it starts and stamps
// `navigation`, `status`, and `snapshot` events with the page and that number.
// The panel applies an event only for the active page, and never one from a
// navigation older than the newest it has seen for that page. A finished-load
// snapshot also has to be newer than the page's floor: the floor rises when a
// panel action is sent (a new load is expected) and when a load is rejected or
// fails. Any native navigation that starts after that raises the newest id
// above the floor, so its result applies again. Tab-selection snapshots
// describe the page as it is right now and always apply.

function pageEntry(state, pageId) {
  let entry = state.get(pageId);
  if (!entry) {
    entry = { latest: 0, floor: 0 };
    state.set(pageId, entry);
  }
  return entry;
}

function navigationIdOf(event) {
  return Number.isInteger(event?.navigationId) ? event.navigationId : null;
}

export function createNativeNavigationState() {
  return new Map();
}

/** A panel navigate/back/forward/reload expects a navigation newer than any seen so far. */
export function notePanelNavigation(state, pageId) {
  if (!Number.isInteger(pageId)) return;
  const entry = pageEntry(state, pageId);
  entry.floor = Math.max(entry.floor, entry.latest);
}

export function forgetNativePage(state, pageId) {
  state.delete(pageId);
}

function loadingStatus(url) {
  return url ? `Loading ${url}…` : "Loading…";
}

/**
 * Returns what the panel should apply for one native event:
 * `{ snapshot }`, `{ status }`, `{ status, url }`, or `{}` to ignore it.
 */
export function nativeStatusUpdate(state, event, activePageId) {
  if (!event) return {};
  if (event.type === "status" && !Number.isInteger(event.pageId)) {
    return { status: event.message };
  }
  if (!["navigation", "status", "snapshot"].includes(event.type)) return {};
  if (!Number.isInteger(event.pageId)) return {};

  const entry = pageEntry(state, event.pageId);
  const id = navigationIdOf(event);
  if (id !== null && id < entry.latest) return {};
  if (id !== null) entry.latest = id;
  if (event.pageId !== activePageId) return {};

  if (event.type === "navigation") {
    return event.url ? { status: loadingStatus(event.url), url: event.url } : { status: loadingStatus("") };
  }
  if (event.type === "status") {
    entry.floor = Math.max(entry.floor, entry.latest);
    return { status: event.message };
  }
  if (event.loading) {
    return { status: loadingStatus(event.snapshot?.url || "") };
  }
  if (event.reason !== "select" && (id ?? 0) <= entry.floor) return {};
  return event.snapshot ? { snapshot: event.snapshot } : {};
}
