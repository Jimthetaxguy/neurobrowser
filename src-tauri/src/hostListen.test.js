import assert from "node:assert/strict";
import { test } from "node:test";
import { bindHostListen } from "./hostListen.js";
import { bindContextCopy } from "./contextCopy.js";
import { bindDownloads } from "./downloads.js";
import { bindMenuCommand } from "./menuCommands.js";
import { bindPageLifecycle, bindPageNewWindow } from "./nativePageEvents.js";

test("cancel before listen resolves unsubscribes and does not keep the listener", async () => {
  let unlistenCount = 0;
  const stop = bindHostListen((name) => {
    assert.equal(name, "host-event");
    return () => {
      unlistenCount += 1;
    };
  }, "host-event", () => {}, "handler failed", "listen failed");
  stop();
  await Promise.resolve();
  await Promise.resolve();
  assert.equal(unlistenCount, 1);
  stop();
  assert.equal(unlistenCount, 1);
});

// Tauri may deliver events before its registration promise resolves, and an
// already queued event may arrive after the native listener is removed.
test("cancel suppresses events while registration is pending and after unlisten", async () => {
  let handler;
  let resolveListen;
  let calls = 0;
  let unlistens = 0;
  const stop = bindHostListen((_name, callback) => {
    handler = callback;
    return new Promise((resolve) => { resolveListen = resolve; });
  }, "host-event", () => { calls += 1; }, "handler failed", "listen failed");
  await Promise.resolve();
  handler({ payload: "active" });
  assert.equal(calls, 1);
  stop();
  handler({ payload: "cancelled but still registering" });
  assert.equal(calls, 1);
  resolveListen(() => { unlistens += 1; });
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(unlistens, 1);
  handler({ payload: "queued after unlisten" });
  assert.equal(calls, 1);
  stop();
  assert.equal(unlistens, 1);
});

test("cleanup is consumed before a throwing unlisten can be retried", async () => {
  let unlistens = 0;
  const failure = new Error("native unlisten failed");
  const stop = bindHostListen(() => () => {
    unlistens += 1;
    throw failure;
  }, "host-event", () => {}, "handler failed", "listen failed");
  await new Promise((resolve) => setImmediate(resolve));
  assert.throws(stop, (error) => error === failure);
  assert.doesNotThrow(stop);
  assert.equal(unlistens, 1);
});

for (const [name, bind, payload] of [
  ["context copy", bindContextCopy, { text: "copy me" }],
  ["downloads", bindDownloads, { id: "download-1", state: "finished" }],
  ["menu", bindMenuCommand, { command: "new-tab" }],
  ["page lifecycle", bindPageLifecycle, { pageId: 1, phase: "finished" }],
  ["new window", bindPageNewWindow, { url: "https://example.com/" }],
]) {
  test(`${name} binder stops delivery immediately on cancellation`, async () => {
    let handler;
    let resolveListen;
    let calls = 0;
    let unlistens = 0;
    const stop = bind((_name, callback) => {
      handler = callback;
      return new Promise((resolve) => { resolveListen = resolve; });
    }, () => { calls += 1; });
    await Promise.resolve();
    handler({ payload });
    assert.equal(calls, 1);
    stop();
    handler({ payload });
    assert.equal(calls, 1);
    resolveListen(() => { unlistens += 1; });
    await new Promise((resolve) => setImmediate(resolve));
    stop();
    assert.equal(unlistens, 1);
  });
}
