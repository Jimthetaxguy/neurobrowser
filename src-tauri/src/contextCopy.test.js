import assert from "node:assert/strict";
import { test } from "node:test";
import { JSDOM } from "jsdom";
import { bindContextCopy, contextCopyText, writeClipboardText } from "./contextCopy.js";

test("context copy reads only the text and ignores a file path", () => {
  assert.equal(contextCopyText(null), null);
  assert.equal(contextCopyText({ text: "" }), null);
  assert.equal(
    contextCopyText({
      payload: {
        text: "https://example.com/next",
        path: "/tmp/evil",
        destination: "/tmp/evil",
        filename: "../../etc/passwd",
      },
    }),
    "https://example.com/next",
  );
  assert.equal(contextCopyText({ text: "cats" }), "cats");
});

test("writeClipboardText sends the text and not a path", async () => {
  const dom = new JSDOM("<!doctype html><body></body>");
  const copied = [];
  dom.window.navigator.clipboard = {
    writeText: async (text) => {
      copied.push(text);
    },
  };
  await writeClipboardText("https://example.com/pic.png", dom.window.document);
  await writeClipboardText("", dom.window.document);
  assert.deepEqual(copied, ["https://example.com/pic.png"]);
  dom.window.close();
});

test("bindContextCopy forwards the shell text", async () => {
  let handler = null;
  const stop = bindContextCopy((name, next) => {
    assert.equal(name, "context-copy");
    handler = next;
    return () => {
      handler = null;
    };
  }, (text) => {
    seen.push(text);
  });
  const seen = [];
  await Promise.resolve();
  await Promise.resolve();
  handler({ payload: { text: "https://example.com/a", path: "/tmp/nope" } });
  handler({ payload: { path: "/tmp/nope" } });
  assert.deepEqual(seen, ["https://example.com/a"]);
  stop();
  assert.equal(handler, null);
});
