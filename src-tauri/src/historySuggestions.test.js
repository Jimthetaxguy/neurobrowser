import assert from "node:assert/strict";
import { test } from "node:test";
import {
  HISTORY_SUGGESTION_LIMIT,
  actionLabel,
  historyQueryFromInput,
  isBlankDocument,
  startPageSites,
  suggestionRows,
} from "./historySuggestions.js";

test("a blank document is an empty address or about:blank", () => {
  for (const url of ["", "   ", null, undefined, "about:blank", "ABOUT:BLANK"]) {
    assert.equal(isBlankDocument(url), true, String(url));
  }
  for (const url of ["https://example.com/", "about:blankevil", "http://localhost/"]) {
    assert.equal(isBlankDocument(url), false, url);
  }
});

test("history queries trim and drop a leading question mark", () => {
  assert.equal(historyQueryFromInput("  cats  "), "cats");
  assert.equal(historyQueryFromInput("?example.com"), "example.com");
  assert.equal(historyQueryFromInput("  ? cats  "), "cats");
  assert.equal(historyQueryFromInput("?"), "");
  assert.equal(historyQueryFromInput(""), "");
  assert.equal(historyQueryFromInput("example.com/docs"), "example.com/docs");
});

test("the first suggestion row names a DuckDuckGo search or a navigation", () => {
  assert.equal(actionLabel("cats"), "Search DuckDuckGo for cats");
  assert.equal(actionLabel("?example.com"), "Search DuckDuckGo for example.com");
  assert.equal(actionLabel("example.com/docs"), "Go to https://example.com/docs");
  assert.equal(actionLabel("https://example.com/a"), "Go to https://example.com/a");
  assert.equal(actionLabel("   "), "");

  const rows = suggestionRows("cats", [
    { url: "https://cats.example/page", title: "Cat care" },
  ]);
  assert.equal(rows[0].kind, "action");
  assert.equal(rows[0].label, "Search DuckDuckGo for cats");
  assert.equal(rows[1].kind, "history");
  assert.equal(rows[1].label, "Cat care");
  assert.equal(rows[1].detail, "cats.example");
  assert.equal(rows[1].url, "https://cats.example/page");
});

test("history rows keep store order, skip blank urls, and stop at eight", () => {
  const hits = [];
  for (let index = 0; index < 10; index += 1) {
    hits.push({ url: `https://site${index}.example/`, title: `Site ${index}` });
  }
  hits.splice(3, 0, { url: "  ", title: "Skipped" });
  const rows = suggestionRows("example.com", hits);
  assert.equal(rows[0].label, "Go to https://example.com");
  const history = rows.filter((row) => row.kind === "history");
  assert.equal(history.length, HISTORY_SUGGESTION_LIMIT);
  assert.equal(history[0].label, "Site 0");
  assert.equal(history[7].label, "Site 7");
  assert.equal(history.some((row) => row.label === "Skipped"), false);

  const untitled = suggestionRows("", [{ url: "https://bare.example/a", title: "  " }]);
  assert.equal(untitled.length, 1);
  assert.equal(untitled[0].label, "bare.example");
  assert.equal(untitled[0].detail, "bare.example");
});

test("the new-tab list shows a title and a host", () => {
  const sites = startPageSites([
    { url: "https://docs.example/guide", title: "Guide" },
    { url: "https://bare.example/", title: "" },
    { url: "", title: "Nope" },
    null,
  ]);
  assert.deepEqual(sites, [
    { url: "https://docs.example/guide", title: "Guide", host: "docs.example" },
    { url: "https://bare.example/", title: "bare.example", host: "bare.example" },
  ]);
});
