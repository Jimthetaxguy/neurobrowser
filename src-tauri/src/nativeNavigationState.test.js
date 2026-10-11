import assert from "node:assert/strict";
import { test } from "node:test";
import {
  createNativeNavigationState,
  forgetNativePage,
  nativeStatusUpdate,
  notePanelNavigation,
} from "./nativeNavigationState.js";

const finish = (pageId, navigationId, title) => ({
  type: "snapshot", pageId, navigationId, reason: "finish", loading: false,
  snapshot: { url: `https://${title}.example/`, title },
});

test("a rejection does not block the next native navigation on that page", () => {
  const state = createNativeNavigationState();
  nativeStatusUpdate(state, { type: "navigation", pageId: 0, navigationId: 1 }, 0);
  const rejected = nativeStatusUpdate(state, { type: "status", pageId: 0, navigationId: 1, message: "Rejected" }, 0);
  assert.deepEqual(rejected, { status: "Rejected" });
  assert.deepEqual(nativeStatusUpdate(state, finish(0, 1, "old"), 0), {});
  nativeStatusUpdate(state, { type: "navigation", pageId: 0, navigationId: 2, url: "https://new.example/" }, 0);
  assert.equal(nativeStatusUpdate(state, finish(0, 2, "new"), 0).snapshot.title, "new");
});

test("results are keyed by page and navigation id", () => {
  const state = createNativeNavigationState();
  nativeStatusUpdate(state, { type: "navigation", pageId: 0, navigationId: 5 }, 0);
  assert.deepEqual(nativeStatusUpdate(state, finish(0, 4, "stale"), 0), {});
  assert.deepEqual(nativeStatusUpdate(state, { type: "status", pageId: 0, navigationId: 4, message: "old" }, 0), {});
  assert.deepEqual(nativeStatusUpdate(state, finish(1, 6, "background"), 0), {});
  assert.deepEqual(
    nativeStatusUpdate(state, { type: "snapshot", pageId: 0, navigationId: 5, reason: "select", loading: true, snapshot: { url: "https://slow.example/" } }, 0),
    { status: "Loading https://slow.example/…" }
  );
  // The background page's newest id was recorded even though it was not shown.
  assert.deepEqual(nativeStatusUpdate(state, finish(1, 3, "older"), 1), {});
});

test("a panel action waits for a newer navigation, and closing a page forgets it", () => {
  const state = createNativeNavigationState();
  nativeStatusUpdate(state, { type: "navigation", pageId: 2, navigationId: 1 }, 2);
  notePanelNavigation(state, 2);
  assert.deepEqual(nativeStatusUpdate(state, finish(2, 1, "before"), 2), {});
  forgetNativePage(state, 2);
  assert.equal(nativeStatusUpdate(state, finish(2, 1, "reopened"), 2).snapshot.title, "reopened");
});

test("a status without a page id is still shown", () => {
  const state = createNativeNavigationState();
  assert.deepEqual(nativeStatusUpdate(state, { type: "status", message: "Host note" }, 0), { status: "Host note" });
  assert.deepEqual(nativeStatusUpdate(state, { type: "tabs", tabs: [] }, 0), {});
});
