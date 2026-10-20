import { bindHostListen } from "./hostListen.js";

/** Main-webview event. Payload is `{ text }` from the shell, never a file path. */
const CONTEXT_COPY_EVENT = "context-copy";

/**
 * The text the shell asked the chrome to copy, or null.
 * A `path`, `destination`, or `filename` on the event is ignored.
 */
export function contextCopyText(event) {
  const payload = event && typeof event === "object" && event.payload && typeof event.payload === "object"
    ? event.payload
    : event;
  if (!payload || typeof payload !== "object") return null;
  if (typeof payload.text !== "string" || payload.text.length === 0) return null;
  return payload.text;
}

/**
 * Put `text` on the clipboard. This does not read a path from the caller.
 * `document.execCommand` covers a page whose clipboard API is missing.
 */
export async function writeClipboardText(text, doc = document) {
  if (typeof text !== "string" || text.length === 0) return;
  const view = doc.defaultView || doc;
  const clipboard = view.navigator && view.navigator.clipboard;
  if (clipboard && typeof clipboard.writeText === "function") {
    await clipboard.writeText(text);
    return;
  }
  const area = doc.createElement("textarea");
  area.value = text;
  doc.body.appendChild(area);
  area.select();
  doc.execCommand("copy");
  area.remove();
}

/**
 * Subscribe with a Tauri-style `listen(name, handler)` that receives `{ payload }`.
 * `callback` receives the text to copy.
 */
export function bindContextCopy(listen, callback) {
  return bindHostListen(listen, CONTEXT_COPY_EVENT, (event) => {
    const text = contextCopyText(event);
    if (!text) return;
    callback(text);
  }, "context copy handler failed", "context copy listen failed");
}
