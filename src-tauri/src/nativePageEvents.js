// A background tab title update must not replace an in-progress URL edit.
export function nativePageUpdates(event, activePageId) {
  if (event.type === "tabs") {
    const updates = { tabs: event.tabs, activePageId: event.activePageId };
    if (event.activePageId !== activePageId) {
      updates.url = event.tabs.find((tab) => tab.id === event.activePageId)?.url || "";
    }
    return updates;
  }
  if (event.type === "snapshot" && event.pageId === activePageId) {
    return { snapshot: event.snapshot };
  }
  return {};
}

/** Main-webview event from the page webview. Payload is `{pageId, phase, url, title}`. */
export const PAGE_LIFECYCLE_EVENT = "page-lifecycle";

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
  let stop = () => {};
  let cancelled = false;
  Promise.resolve()
    .then(() => listen(PAGE_LIFECYCLE_EVENT, (event) => {
      const payload = event && typeof event === "object" && event.payload && typeof event.payload === "object"
        ? event.payload
        : event;
      try {
        callback(payload);
      } catch (error) {
        console.error("page lifecycle handler failed", error);
      }
    }))
    .then((unsubscribe) => {
      if (typeof unsubscribe !== "function") return;
      if (cancelled) unsubscribe();
      else stop = unsubscribe;
    })
    .catch((error) => {
      console.error("page lifecycle listen failed", error);
    });
  return () => {
    cancelled = true;
    stop();
    stop = () => {};
  };
}
