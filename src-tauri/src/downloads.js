/** Main-webview event for one download row. */
export const DOWNLOAD_EVENT = "download-update";

const DOWNLOAD_PHASES = new Set(["progress", "done", "failed"]);
const DOWNLOAD_LIMIT = 20;

/**
 * Merge one download event into the chrome list.
 * A late progress tick cannot turn a finished row back into a download.
 */
export function applyDownloadUpdate(list, event) {
  const currentList = Array.isArray(list) ? list : [];
  if (!event || typeof event !== "object") return currentList;
  if (typeof event.id !== "string" || event.id === "") return currentList;
  if (!DOWNLOAD_PHASES.has(event.phase)) return currentList;
  if (typeof event.filename !== "string" || event.filename === "") return currentList;
  const row = {
    id: event.id,
    phase: event.phase,
    url: typeof event.url === "string" ? event.url : "",
    filename: event.filename,
    received: Number.isFinite(event.received) ? event.received : 0,
    total: Number.isFinite(event.total) ? event.total : null,
    error: typeof event.error === "string" && event.error ? event.error : "",
  };
  const index = currentList.findIndex((item) => item.id === row.id);
  if (index === -1) {
    return [row, ...currentList].slice(0, DOWNLOAD_LIMIT);
  }
  const current = currentList[index];
  if (row.phase === "progress" && (current.phase === "done" || current.phase === "failed")) {
    return currentList;
  }
  const next = currentList.slice();
  next[index] = row;
  return next;
}

/** "Downloading", a byte count, or a percent when the size is known. */
export function downloadProgressText(item) {
  const received = Number(item?.received) || 0;
  const total = Number(item?.total);
  if (Number.isFinite(total) && total > 0) {
    return `${Math.min(100, Math.round((received / total) * 100))}%`;
  }
  if (received > 0) return `${received} bytes`;
  return "Downloading";
}

/**
 * Subscribe with a Tauri-style `listen(name, handler)` that receives `{ payload }`.
 * `callback` receives one download row.
 */
export function bindDownloads(listen, callback) {
  let stop = () => {};
  let cancelled = false;
  Promise.resolve()
    .then(() => listen(DOWNLOAD_EVENT, (event) => {
      const payload = event && typeof event === "object" && event.payload && typeof event.payload === "object"
        ? event.payload
        : event;
      try {
        callback(payload);
      } catch (error) {
        console.error("download handler failed", error);
      }
    }))
    .then((unsubscribe) => {
      if (typeof unsubscribe !== "function") return;
      if (cancelled) unsubscribe();
      else stop = unsubscribe;
    })
    .catch((error) => {
      console.error("download listen failed", error);
    });
  return () => {
    cancelled = true;
    stop();
    stop = () => {};
  };
}
