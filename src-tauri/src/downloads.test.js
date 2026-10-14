import assert from "node:assert/strict";
import { test } from "node:test";
import { applyDownloadUpdate, downloadProgressText } from "./downloads.js";

const row = (phase, extra = {}) => ({
  id: "dl-1",
  phase,
  url: "https://cdn.example/report.pdf",
  filename: "report.pdf",
  received: 0,
  total: null,
  ...extra,
});

test("download rows move from progress to done or failed and ignore a late tick", () => {
  let list = applyDownloadUpdate([], row("progress"));
  assert.equal(list.length, 1);
  assert.equal(downloadProgressText(list[0]), "Downloading");

  list = applyDownloadUpdate(list, row("progress", { received: 50, total: 200 }));
  assert.equal(downloadProgressText(list[0]), "25%");
  assert.equal(list[0].phase, "progress");

  list = applyDownloadUpdate(list, row("done", { received: 200, total: 200 }));
  assert.equal(list[0].phase, "done");

  const stuck = applyDownloadUpdate(list, row("progress", { received: 10, total: 200 }));
  assert.equal(stuck[0].phase, "done");
  assert.equal(stuck, list);

  list = applyDownloadUpdate(list, row("failed", { id: "dl-2", filename: "secret.bin", error: "Refusing to fetch internal/loopback address" }));
  assert.equal(list[0].id, "dl-2");
  assert.equal(list[0].phase, "failed");
  assert.equal(list[1].phase, "done");
});

test("a download event without an id or a phase is ignored", () => {
  const list = [row("done")];
  assert.equal(applyDownloadUpdate(list, { phase: "progress", filename: "a" }), list);
  assert.equal(applyDownloadUpdate(list, { id: "dl-9", phase: "nope", filename: "a" }), list);
  assert.equal(downloadProgressText({ received: 12 }), "12 bytes");
});
