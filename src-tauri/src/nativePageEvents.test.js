import assert from "node:assert/strict";
import { test } from "node:test";
import { nativePageUpdates } from "./nativePageEvents.js";

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
