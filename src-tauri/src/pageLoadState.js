// Per-page load state. The shell shows the active page's phase, but events
// arrive for every page, so each page keeps its own entry.
//
// An entry is `{ native, command }`:
// - `native` is true between a page-lifecycle `started` and `finished`.
// - `command` is the token of a shell-issued load (navigate, open from opener)
//   that has not returned yet, or null.
// A page with no entry is idle. Functions return the same object when nothing changes.

function entryOf(loads, pageId) {
  return loads && Object.hasOwn(loads, pageId) ? loads[pageId] : null;
}

function withEntry(loads, pageId, entry) {
  const next = { ...(loads || {}) };
  if (!entry || (!entry.native && entry.command == null)) delete next[pageId];
  else next[pageId] = entry;
  return next;
}

/** Apply a page-lifecycle phase for any page, active or not. */
export function markLifecycle(loads, pageId, phase) {
  if (!Number.isInteger(pageId)) return loads;
  if (phase !== "started" && phase !== "finished") return loads;
  const entry = entryOf(loads, pageId);
  const native = phase === "started";
  if ((entry?.native ?? false) === native) return loads;
  return withEntry(loads, pageId, { native, command: entry?.command ?? null });
}

/** Record a shell-issued load. `token` must be unique per call. */
export function beginCommandLoad(loads, pageId, token) {
  if (!Number.isInteger(pageId)) return loads;
  const entry = entryOf(loads, pageId);
  return withEntry(loads, pageId, { native: entry?.native ?? false, command: token });
}

/**
 * End a shell-issued load. Only the latest token for that page ends it, so a
 * superseded navigation that returns late cannot clear a newer one. The host
 * command returns after the load settles, so the lifecycle phase settles too.
 */
export function endCommandLoad(loads, pageId, token) {
  const entry = entryOf(loads, pageId);
  if (!entry || entry.command !== token) return loads;
  return withEntry(loads, pageId, null);
}

/** Stop or close: the page is idle. */
export function clearPageLoad(loads, pageId) {
  if (!entryOf(loads, pageId)) return loads;
  return withEntry(loads, pageId, null);
}

/** `"loading"` or `"idle"`. */
export function pageLoadPhase(loads, pageId) {
  const entry = entryOf(loads, pageId);
  return entry && (entry.native || entry.command != null) ? "loading" : "idle";
}

export function isPageLoading(loads, pageId) {
  return pageLoadPhase(loads, pageId) === "loading";
}
