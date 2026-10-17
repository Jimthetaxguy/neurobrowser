/**
 * Tauri `listen(name, handler)` that resolves to an unlisten function.
 * A non-function unlisten is ignored. If the caller already cancelled,
 * the unlisten runs immediately instead of being kept.
 * Throws from `onEvent` are logged with `handlerFailed`.
 * A rejected listen is logged with `listenFailed`.
 */
export function bindHostListen(listen, name, onEvent, handlerFailed, listenFailed) {
  let stop = () => {};
  let cancelled = false;
  Promise.resolve()
    .then(() => listen(name, (event) => {
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
    stop();
    stop = () => {};
  };
}
