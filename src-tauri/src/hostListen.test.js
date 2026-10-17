import assert from "node:assert/strict";
import { test } from "node:test";
import { bindHostListen } from "./hostListen.js";

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
