import assert from "node:assert/strict";
import { test } from "node:test";
import {
  bindPageLifecycle,
  moveTab,
  nativePageUpdates,
  pageLifecycleUpdates,
  securityState,
} from "./nativePageEvents.js";

test("background tab updates preserve the active page URL draft", () => {
  const update = nativePageUpdates({
    type: "tabs", activePageId: 0,
    tabs: [{ id: 0, url: "https://active.example" }, { id: -1, title: "Finished loading" }],
  }, 0);
  assert.equal(Object.hasOwn(update, "url"), false);
  assert.equal(update.tabs[1].title, "Finished loading");
});

test("a native selection and immediate snapshot use the new active page", () => {
  const selection = nativePageUpdates({
    type: "tabs", activePageId: -1,
    tabs: [{ id: 0 }, { id: -1, url: "https://selected.example" }],
  }, 0);
  assert.equal(selection.url, "https://selected.example");
  const snapshot = { title: "Selected", url: selection.url };
  assert.deepEqual(nativePageUpdates({ type: "snapshot", pageId: -1, snapshot }, selection.activePageId), { snapshot });
  assert.deepEqual(nativePageUpdates({ type: "snapshot", pageId: 0, snapshot }, selection.activePageId), {});
});

test("page lifecycle follows the active page and keeps a background title off the address bar", () => {
  const started = pageLifecycleUpdates({
    pageId: 2,
    phase: "started",
    url: "http://inpage.example/next",
    title: "",
  }, 2);
  assert.equal(started.loading, true);
  assert.equal(started.url, "http://inpage.example/next");
  assert.equal(Object.hasOwn(started, "title"), false);

  const title = pageLifecycleUpdates({
    pageId: 2,
    phase: "title",
    url: "http://inpage.example/next",
    title: "In-page",
  }, 2);
  assert.equal(title.title, "In-page");
  assert.equal(title.url, "http://inpage.example/next");

  const finished = pageLifecycleUpdates({
    pageId: 2,
    phase: "finished",
    url: "https://inpage.example/landed",
    title: "",
  }, 2);
  assert.equal(finished.loading, false);
  assert.equal(finished.url, "https://inpage.example/landed");
  assert.equal(Object.hasOwn(finished, "title"), false);

  const background = pageLifecycleUpdates({
    pageId: 9,
    phase: "title",
    url: "https://other.example/",
    title: "Background",
  }, 2);
  assert.equal(background.title, "Background");
  assert.equal(Object.hasOwn(background, "url"), false);
  assert.equal(Object.hasOwn(background, "loading"), false);
});

test("an address draft blocks lifecycle URL updates and unknown payloads are ignored", () => {
  const draft = pageLifecycleUpdates({
    pageId: 0,
    phase: "started",
    url: "https://replaced.example/",
    title: "Still titled",
  }, 0, { addressEditing: true });
  assert.equal(Object.hasOwn(draft, "url"), false);
  assert.equal(draft.title, "Still titled");
  assert.equal(draft.loading, true);
  assert.deepEqual(pageLifecycleUpdates({ pageId: 0, phase: "paint", url: "https://x", title: "X" }, 0), {});
  assert.deepEqual(pageLifecycleUpdates(null, 0), {});
});

test("security state distinguishes http and https", () => {
  assert.equal(securityState("https://example.test/a"), "secure");
  assert.equal(securityState(" HTTPS://example.test "), "secure");
  assert.equal(securityState("http://example.test"), "insecure");
  assert.equal(securityState("http://127.0.0.1/"), "insecure");
  assert.equal(securityState("about:blank"), "unknown");
  assert.equal(securityState(""), "unknown");
});

test("tabs move by drag index and ignore a no-op drop", () => {
  const tabs = [{ id: 0 }, { id: 1 }, { id: 2 }];
  assert.deepEqual(moveTab(tabs, 0, 2).map((tab) => tab.id), [1, 2, 0]);
  assert.deepEqual(moveTab(tabs, 2, 0).map((tab) => tab.id), [2, 0, 1]);
  assert.equal(moveTab(tabs, 1, 1), tabs);
  assert.equal(moveTab(tabs, -1, 0), tabs);
});

test("bindPageLifecycle forwards a dispatched page-lifecycle payload", async () => {
  let handler = null;
  const listen = (name, next) => {
    assert.equal(name, "page-lifecycle");
    handler = next;
    return () => { handler = null; };
  };
  const seen = [];
  const stop = bindPageLifecycle(listen, (payload) => seen.push(payload));
  await Promise.resolve();
  await Promise.resolve();
  const payload = { pageId: 4, phase: "started", url: "http://example.test", title: "" };
  handler({ payload });
  assert.deepEqual(seen, [payload]);
  stop();
  assert.equal(handler, null);
});
