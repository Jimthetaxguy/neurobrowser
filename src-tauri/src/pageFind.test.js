import assert from "node:assert/strict";
import { test } from "node:test";
import { createElement } from "react";
import { createRoot } from "react-dom/client";
import { JSDOM } from "jsdom";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";
import { FindBar, chromeShortcut, findMatchLabel } from "./pageFind.js";

const here = path.dirname(fileURLToPath(import.meta.url));
const findFunction = readFileSync(path.join(here, "pageFindFunction.js"), "utf8");

function findIn(html, query, index, withSelection = false) {
  const dom = new JSDOM(`<!doctype html><html><body>${html}</body></html>`, {
    url: "https://example.test/page",
    runScripts: "outside-only",
  });
  try {
    dom.window.eval(`${findFunction}\nwindow.__find = neurobrowserFind;`);
    const result = dom.window.__find(query, index);
    return { total: Number(result.total), current: Number(result.current),
      ...(withSelection ? { selection: dom.window.getSelection().toString() } : {}) };
  } finally {
    dom.window.close();
  }
}

test("find counts matches, wraps, and skips script text", () => {
  const page = "<p>Cats and cats.</p><script>cats</script><style>cats</style>";
  assert.deepEqual(findIn(page, "cats", 0), { total: 2, current: 1 });
  assert.deepEqual(findIn(page, "cats", 1), { total: 2, current: 2 });
  assert.deepEqual(findIn(page, "cats", 2), { total: 2, current: 1 });
  assert.deepEqual(findIn(page, "missing", 0), { total: 0, current: 0 });
  assert.deepEqual(findIn(page, "", 0), { total: 0, current: 0 });
});

test("the match label names the current hit", () => {
  assert.equal(findMatchLabel(1, 12), "1 of 12");
  assert.equal(findMatchLabel(0, 0), "No matches");
  assert.equal(findMatchLabel(3, 0), "No matches");
});

test("find and zoom shortcuts accept command and control, plus and minus", () => {
  assert.equal(chromeShortcut({ key: "f", metaKey: true }), "find");
  assert.equal(chromeShortcut({ key: "f", ctrlKey: true }), "find");
  assert.equal(chromeShortcut({ key: "F", ctrlKey: true, shiftKey: true }), null);
  assert.equal(chromeShortcut({ key: "g", metaKey: true }), "find-next");
  assert.equal(chromeShortcut({ key: "G", metaKey: true, shiftKey: true }), "find-previous");
  assert.equal(chromeShortcut({ key: "=", metaKey: true }), "zoom-in");
  assert.equal(chromeShortcut({ key: "+", metaKey: true, shiftKey: true }), "zoom-in");
  assert.equal(chromeShortcut({ key: "+", code: "NumpadAdd", ctrlKey: true }), "zoom-in");
  assert.equal(chromeShortcut({ key: "-", ctrlKey: true }), "zoom-out");
  assert.equal(chromeShortcut({ key: "_", code: "Minus", ctrlKey: true, shiftKey: true }), null);
  assert.equal(chromeShortcut({ key: "-", code: "NumpadSubtract", metaKey: true }), "zoom-out");
  assert.equal(chromeShortcut({ key: "0", metaKey: true }), "zoom-reset");
  assert.equal(chromeShortcut({ key: "0", metaKey: true, shiftKey: true }), null);
  assert.equal(chromeShortcut({ key: "f" }), null);
  assert.equal(chromeShortcut({ key: "f", metaKey: true, altKey: true }), null);
  assert.equal(chromeShortcut({ key: "t", metaKey: true }), null);
});

test("the find bar shows the count and moves with the buttons", async () => {
  const dom = new JSDOM("<div id='root'></div>", { url: "https://shell.example" });
  for (const name of ["Element", "HTMLElement", "HTMLInputElement"]) {
    const proto = dom.window[name]?.prototype;
    if (!proto || typeof proto.attachEvent === "function") continue;
    proto.attachEvent = () => {};
    proto.detachEvent = () => {};
  }
  const previous = new Map();
  for (const [key, value] of Object.entries({
    window: dom.window,
    document: dom.window.document,
    IS_REACT_ACT_ENVIRONMENT: true,
  })) {
    previous.set(key, Object.getOwnPropertyDescriptor(globalThis, key));
    Object.defineProperty(globalThis, key, { configurable: true, writable: true, value });
  }
  const { act } = await import("react");
  const root = createRoot(document.getElementById("root"));
  const seen = [];
  try {
    await act(async () => {
      root.render(createElement(FindBar, {
        query: "cats",
        total: 4,
        current: 2,
        onQuery: (value) => seen.push(["query", value]),
        onNext: () => seen.push(["next"]),
        onPrevious: () => seen.push(["previous"]),
        onClose: () => seen.push(["close"]),
      }));
    });
    assert.equal(document.querySelector(".find-count").textContent, "2 of 4");
    document.querySelector("[aria-label='Next match']").click();
    document.querySelector("[aria-label='Previous match']").click();
    document.querySelector("[aria-label='Close find']").click();
    const input = document.querySelector("[aria-label='Find']");
    const setter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, "value").set;
    await act(async () => {
      const tracker = input._valueTracker;
      if (tracker) tracker.setValue("cats");
      setter.call(input, "dog");
      input.dispatchEvent(new window.Event("input", { bubbles: true }));
    });
    assert.deepEqual(seen, [["next"], ["previous"], ["close"], ["query", "dog"]]);
  } finally {
    await act(async () => { root.unmount(); });
    for (const [key, descriptor] of previous) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor);
      else delete globalThis[key];
    }
    dom.window.close();
  }
});

test("find uses original DOM offsets after case-expanding Unicode text", () => {
  for (const [index, selection] of [[0, "cats"], [1, "CATS"]]) {
    assert.deepEqual(findIn("<p>İ cats CATS</p>", "cats", index, true),
      { total: 2, current: index + 1, selection });
  }
  assert.deepEqual(findIn("<p>İx</p>", "İ", 0, true),
    { total: 1, current: 1, selection: "İ" });
});

test("find treats regex syntax literally and preserves astral text offsets", () => {
  assert.deepEqual(findIn("<p>😀 [a+b].* 😀 [A+B].*</p>", "[a+b].*", 1, true),
    { total: 2, current: 2, selection: "[A+B].*" });
});

test("find matches Greek sigma case variants at their original offsets", () => {
  assert.deepEqual(findIn("<p>ΟΣ ος οσ</p>", "οσ", 1, true),
    { total: 3, current: 2, selection: "ος" });
});
