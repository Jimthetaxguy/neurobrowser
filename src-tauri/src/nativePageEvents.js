import { bindHostListen } from "./hostListen.js";

// A background tab title update must not replace an in-progress URL edit.
export function nativePageUpdates(event, activePageId) {
  if (event.type === "tabs") {
    const updates = { tabs: event.tabs, activePageId: event.activePageId };
    if (event.activePageId !== activePageId) {
      updates.url = event.tabs.find((tab) => tab.id === event.activePageId)?.url || "";
    }
    return updates;
  }
  return {};
}

/** Main-webview event from the page webview. Payload is `{pageId, phase, url, title}`. */
export const PAGE_LIFECYCLE_EVENT = "page-lifecycle";

/** Main-webview event for a popup or new browsing context. Payload is `{pageId, url, disposition}`. */
export const PAGE_NEW_WINDOW_EVENT = "page-new-window";

const NEW_WINDOW_DISPOSITIONS = new Set(["foreground", "background", "blocked"]);

/**
 * One new-window payload, or null when it is not a shell request.
 * `pageId` is the opener. An empty `url` is a blank tab.
 */
export function pageNewWindowRequest(event) {
  if (!event || typeof event !== "object") return null;
  if (!Number.isInteger(event.pageId) || event.pageId < 0) return null;
  if (!NEW_WINDOW_DISPOSITIONS.has(event.disposition)) return null;
  const url = typeof event.url === "string" ? event.url : "";
  return { pageId: event.pageId, url, disposition: event.disposition };
}

/** Insert the opened tab beside its opener. The opener id stays on the tab. */
export function insertOpenedTab(tabs, openerId, pageId, url) {
  const tab = { id: pageId, title: "New tab", url: url || "", openerId };
  const next = Array.isArray(tabs) ? tabs.slice() : [];
  const openerIndex = next.findIndex((item) => item.id === openerId);
  next.splice(openerIndex === -1 ? next.length : openerIndex + 1, 0, tab);
  return next;
}

/**
 * Which tab should be visible after a close.
 * Closing the active tab returns to its opener when that tab is still open.
 * Otherwise the previous neighbor rule applies.
 */
export function focusAfterClose(tabs, closingId, activeId) {
  if (!Array.isArray(tabs) || tabs.length === 0) return null;
  const closing = tabs.find((tab) => tab.id === closingId);
  const remaining = tabs.filter((tab) => tab.id !== closingId);
  if (remaining.length === 0) return null;
  if (closingId === activeId && closing && remaining.some((tab) => tab.id === closing.openerId)) {
    return closing.openerId;
  }
  const closingIndex = tabs.findIndex((tab) => tab.id === closingId);
  const next = remaining[closingIndex] || remaining[closingIndex - 1] || remaining[0];
  return next ? next.id : null;
}

const BLOCKED_POPUP_LIMIT = 8;

/** Keep the latest blocked popups for one opener. */
export function rememberBlockedPopup(byPage, pageId, url) {
  const next = { ...(byPage || {}) };
  const list = Array.isArray(next[pageId]) ? next[pageId].slice() : [];
  list.push(url);
  next[pageId] = list.slice(-BLOCKED_POPUP_LIMIT);
  return next;
}

/** Take the oldest blocked popup for one opener. */
export function takeBlockedPopup(byPage, pageId) {
  const list = Array.isArray(byPage?.[pageId]) ? byPage[pageId].slice() : [];
  if (list.length === 0) return { popups: byPage || {}, url: null };
  const url = list.shift();
  const next = { ...byPage };
  if (list.length > 0) next[pageId] = list;
  else delete next[pageId];
  return { popups: next, url };
}

export function forgetBlockedPopups(byPage, pageId) {
  if (!byPage || !Object.hasOwn(byPage, pageId)) return byPage || {};
  const next = { ...byPage };
  delete next[pageId];
  return next;
}

export function blockedPopupLabel(count) {
  if (!count) return "";
  if (count === 1) return "Popup blocked";
  return `${count} popups blocked`;
}

const PAGE_LIFECYCLE_PHASES = new Set(["started", "finished", "title"]);

/**
 * Chrome updates for one page-lifecycle payload.
 * The address bar follows the active page unless that text is an unsent edit.
 * A non-empty title updates that page's tab, including a background tab.
 * `loading` is reported only for the active page.
 */
export function pageLifecycleUpdates(event, activePageId, options = {}) {
  if (!event || typeof event !== "object") return {};
  const pageId = event.pageId;
  const phase = event.phase;
  if (!Number.isInteger(pageId) || !PAGE_LIFECYCLE_PHASES.has(phase)) return {};

  const updates = { pageId, phase };
  const active = pageId === activePageId;
  if (active && phase === "started") updates.loading = true;
  if (active && phase === "finished") updates.loading = false;

  const addressEditing = options.addressEditing === true;
  if (active && !addressEditing && typeof event.url === "string" && event.url) {
    updates.url = event.url;
  }
  if (typeof event.title === "string" && event.title.trim()) {
    updates.title = event.title;
  }
  return updates;
}

/** `secure` for https, `insecure` for http, `unknown` otherwise. */
export function securityState(url) {
  const value = String(url ?? "").trim();
  if (/^https:\/\//i.test(value)) return "secure";
  if (/^http:\/\//i.test(value)) return "insecure";
  return "unknown";
}

/** Move a tab from one index to another. The same array is returned when nothing moves. */
export function moveTab(tabs, fromIndex, toIndex) {
  if (!Array.isArray(tabs)) return [];
  if (!Number.isInteger(fromIndex) || !Number.isInteger(toIndex)) return tabs;
  if (fromIndex < 0 || toIndex < 0 || fromIndex >= tabs.length || toIndex >= tabs.length) return tabs;
  if (fromIndex === toIndex) return tabs;
  const next = tabs.slice();
  const [moved] = next.splice(fromIndex, 1);
  next.splice(toIndex, 0, moved);
  return next;
}

/**
 * Subscribe with a Tauri-style `listen(name, handler)` that receives `{ payload }`
 * and resolves to an unlisten function. `callback` receives `{pageId, phase, url, title}`.
 */
export function bindPageLifecycle(listen, callback) {
  return bindHostListen(listen, PAGE_LIFECYCLE_EVENT, (event) => {
    const payload = event && typeof event === "object" && event.payload && typeof event.payload === "object"
      ? event.payload
      : event;
    callback(payload);
  }, "page lifecycle handler failed", "page lifecycle listen failed");
}

/** Same listen shape as {@link bindPageLifecycle}, for `page-new-window`. */
export function bindPageNewWindow(listen, callback) {
  return bindHostListen(listen, PAGE_NEW_WINDOW_EVENT, (event) => {
    const payload = event && typeof event === "object" && event.payload && typeof event.payload === "object"
      ? event.payload
      : event;
    callback(payload);
  }, "page new window handler failed", "page new window listen failed");
}
