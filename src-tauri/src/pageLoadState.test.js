import assert from "node:assert/strict";
import { test } from "node:test";
import {
  beginCommandLoad,
  clearPageLoad,
  endCommandLoad,
  isPageLoading,
  markLifecycle,
  pageLoadPhase,
} from "./pageLoadState.js";

test("lifecycle phases are tracked per page, including background pages", () => {
  let loads = {};
  loads = markLifecycle(loads, 0, "started");
  loads = markLifecycle(loads, 1, "started");
  assert.equal(pageLoadPhase(loads, 0), "loading");
  assert.equal(pageLoadPhase(loads, 1), "loading");

  // Tab 0 is in the background when it finishes. Its phase still clears.
  loads = markLifecycle(loads, 0, "finished");
  assert.equal(isPageLoading(loads, 0), false);
  assert.equal(isPageLoading(loads, 1), true);
  assert.equal(pageLoadPhase(loads, 7), "idle");
});

test("a title event does not change the load phase", () => {
  const loads = markLifecycle({}, 3, "started");
  assert.equal(markLifecycle(loads, 3, "title"), loads);
  assert.equal(markLifecycle({}, 3, "title").hasOwnProperty(3), false);
});

test("invalid input leaves the state unchanged", () => {
  const loads = markLifecycle({}, 1, "started");
  assert.equal(markLifecycle(loads, "1", "finished"), loads);
  assert.equal(markLifecycle(loads, 1, "paint"), loads);
  assert.equal(isPageLoading(null, 1), false);
});

test("a command load ends only for its own token and page", () => {
  let loads = beginCommandLoad({}, 0, 1);
  loads = beginCommandLoad(loads, 0, 2);
  loads = beginCommandLoad(loads, 4, 3);
  assert.equal(isPageLoading(loads, 0), true);

  // A superseded navigation finishing must not clear the newer one.
  loads = endCommandLoad(loads, 0, 1);
  assert.equal(isPageLoading(loads, 0), true);
  loads = endCommandLoad(loads, 0, 2);
  assert.equal(isPageLoading(loads, 0), false);
  assert.equal(isPageLoading(loads, 4), true);
});

test("a finished lifecycle keeps an outstanding command load", () => {
  let loads = beginCommandLoad({}, 0, 5);
  loads = markLifecycle(loads, 0, "started");
  loads = markLifecycle(loads, 0, "finished");
  assert.equal(isPageLoading(loads, 0), true);
  loads = endCommandLoad(loads, 0, 5);
  assert.equal(isPageLoading(loads, 0), false);
});

test("ending a command load also settles the page's lifecycle phase", () => {
  let loads = beginCommandLoad({}, 2, 9);
  loads = markLifecycle(loads, 2, "started");
  loads = endCommandLoad(loads, 2, 9);
  assert.equal(isPageLoading(loads, 2), false);
});

test("stop or close clears one page only", () => {
  let loads = markLifecycle({}, 0, "started");
  loads = beginCommandLoad(loads, 0, 1);
  loads = markLifecycle(loads, 1, "started");
  loads = clearPageLoad(loads, 0);
  assert.equal(isPageLoading(loads, 0), false);
  assert.equal(isPageLoading(loads, 1), true);
  assert.equal(clearPageLoad(loads, 99), loads);
});
