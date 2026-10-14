/** Event the native menu emits to the main control webview only. */
export const MENU_COMMAND_EVENT = "menu-command";

/**
 * A menu accelerator and the chrome keydown listener can both deliver one
 * physical key. The menu event arrives over IPC after the keydown handler, so
 * a short window is enough to drop that echo without swallowing a later click
 * of the same menu item.
 */
export const MENU_ECHO_MS = 200;

export function menuCommandFromEvent(event) {
  const payload = event && typeof event === "object" && "payload" in event ? event.payload : event;
  if (typeof payload === "string" && payload) return payload;
  if (payload && typeof payload === "object" && typeof payload.command === "string" && payload.command) {
    return payload.command;
  }
  return null;
}

/** `select-tab-1` … `select-tab-9`, or null. Nine means the last tab. */
export function numberedTabCommand(command) {
  if (typeof command !== "string") return null;
  const match = /^select-tab-([1-9])$/.exec(command);
  if (!match) return null;
  return Number(match[1]);
}

export function createMenuEchoGuard(now = () => Date.now()) {
  const marks = new Map();
  return {
    mark(command, at = now()) {
      if (typeof command !== "string" || command === "") return;
      marks.set(command, at);
    },
    isEcho(command, at = now()) {
      const marked = marks.get(command);
      return typeof marked === "number" && at >= marked && at - marked < MENU_ECHO_MS;
    },
  };
}

/**
 * Subscribe with a Tauri-style `listen(name, handler)` that receives `{ payload }`.
 * `callback` receives the command string (`"new-tab"`, `"back"`, …).
 */
export function bindMenuCommand(listen, callback) {
  let stop = () => {};
  let cancelled = false;
  Promise.resolve()
    .then(() => listen(MENU_COMMAND_EVENT, (event) => {
      const command = menuCommandFromEvent(event);
      if (!command) return;
      try {
        callback(command);
      } catch (error) {
        console.error("menu command handler failed", error);
      }
    }))
    .then((unsubscribe) => {
      if (typeof unsubscribe !== "function") return;
      if (cancelled) unsubscribe();
      else stop = unsubscribe;
    })
    .catch((error) => {
      console.error("menu command listen failed", error);
    });
  return () => {
    cancelled = true;
    stop();
    stop = () => {};
  };
}
