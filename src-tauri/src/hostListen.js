/**
 * Tauri `listen(name, handler)` that resolves to an unlisten function.
 * A non-function unlisten is ignored. If the caller already cancelled,
 * the unlisten runs immediately instead of being kept.
 * Events after cancellation are ignored, including queued native events.
 * Throws from `onEvent` are logged with `handlerFailed`.
 * A rejected listen is logged with `listenFailed`.
 */
export function bindHostListen(listen, name, onEvent, handlerFailed, listenFailed) {
  let stop = () => {};
  let cancelled = false;
  Promise.resolve()
    .then(() => listen(name, (event) => {
      if (cancelled) return;
      try {
        onEvent(event);
      } catch (error) {
        console.error(handlerFailed, error);
      }
    }))
    .then((unsubscribe) => {
      if (typeof unsubscribe !== "function") return;
      if (cancelled) unsubscribe();
      else stop = unsubscribe;
    })
    .catch((error) => {
      console.error(listenFailed, error);
    });
  return () => {
    cancelled = true;
    const unsubscribe = stop;
    stop = () => {};
    unsubscribe();
  };
}
