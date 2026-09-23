import assert from "node:assert/strict";
import { test } from "node:test";
import { createAppKitHostAdapter } from "./hostAdapters.js";

test("AppKit keeps snapshots associated with their page and discards closed pages", async () => {
  const bridge = new EventTarget();
  const sent = [];
  bridge.webkit = { messageHandlers: { neurobrowser: { postMessage: (message) => sent.push(message) } } };
  const previousWindow = globalThis.window;
  globalThis.window = bridge;
  try {
    const adapter = createAppKitHostAdapter();
    const sessionId = await adapter.createSession();
    const reactPage = await adapter.createPage(sessionId);
    const first = { url: "https://first.example", title: "First" };
    const native = { url: "https://native.example", title: "Native" };
    bridge.neurobrowserNativeDispatch({ type: "snapshot", pageId: reactPage, snapshot: first });
    bridge.neurobrowserNativeDispatch({ type: "snapshot", pageId: -1, snapshot: native });
    assert.deepEqual(await adapter.getPageSnapshot(sessionId, reactPage), first);
    assert.deepEqual(await adapter.getPageSnapshot(sessionId, -1), native);
    assert.equal(await adapter.getPageSnapshot(sessionId, 999), null);

    const events = [];
    const unsubscribe = adapter.onHostEvent((event) => events.push(event));
    bridge.neurobrowserNativeDispatch({ type: "tabs", tabs: [{ id: -1 }], activePageId: -1 });
    assert.equal(await adapter.getPageSnapshot(sessionId, reactPage), null);
    assert.deepEqual(await adapter.getPageSnapshot(sessionId, -1), native);
    assert.equal(events[0].activePageId, -1);
    unsubscribe();

    const nextReactPage = await adapter.createPage(sessionId);
    assert.equal(nextReactPage, 1);
    await adapter.navigate(sessionId, -1, native.url);
    await adapter.browserAction("browser_back", sessionId, -1);
    await adapter.closePage(sessionId, -1);
    assert.deepEqual(sent.slice(-3).map(({ command, payload }) => [command, payload.pageId]), [
      ["navigate", -1], ["browser_back", -1], ["close_page", -1],
    ]);
  } finally {
    if (previousWindow === undefined) delete globalThis.window;
    else globalThis.window = previousWindow;
  }
});
