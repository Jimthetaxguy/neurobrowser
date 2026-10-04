import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import { act, createElement, StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { JSDOM } from "jsdom";
import { createServer } from "vite";

let server;
let EvidencePanel;
before(async () => {
  server = await createServer({ server: { middlewareMode: true, watch: null, hmr: false, ws: false }, appType: "custom" });
  ({ EvidencePanel } = await server.ssrLoadModule("/src/EvidencePanel.jsx"));
});
after(async () => { await server?.close(); });

function deferred() {
  let resolve, reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
function observed(pageId, revision = 1) {
  return {
    title: `Reviewed page ${pageId}`, url: `https://page${pageId}.example`, text: `Evidence for page ${pageId}`,
    tables: [], omissions: ["Raw HTML and input values excluded"],
    document: { runtime_id: `page${pageId}`, document_id: `document${pageId}`, revision },
    capabilities: { javascript: true, interaction: true, scoped_targets: true, screenshots: false, enforcing_subresource_network: false },
    targets: [{ id: `target-${pageId}`, role: "button", label: "Dispatch", tag: "button", disabled: false, sensitive: false, destination: null }],
  };
}
async function withPanel(check, { observation, dispatch, cancel, strict = false } = {}) {
  const dom = new JSDOM("<div id='root'></div>", { url: "https://shell.example" });
  const previous = new Map();
  for (const [key, value] of Object.entries({ window: dom.window, document: dom.window.document, IS_REACT_ACT_ENVIRONMENT: true })) {
    previous.set(key, Object.getOwnPropertyDescriptor(globalThis, key));
    Object.defineProperty(globalThis, key, { configurable: true, writable: true, value });
  }
  const calls = [];
  const results = [];
  const adapter = {
    getPageObservation: async (sessionId, pageId) => {
      calls.push(["observe", sessionId, pageId]);
      return observation ? observation(pageId) : observed(pageId);
    },
    executeBrowserTool: async (sessionId, pageId, command) => {
      calls.push(["dispatch", sessionId, pageId, command]);
      return dispatch ? dispatch(pageId, command) : { status: "completed", events: [] };
    },
    cancelAgentRun: async runId => {
      calls.push(["cancel", runId]);
      if (cancel) return cancel(runId);
      return { run_id: runId, status: "cancelled", final_response: "Prior-page approval cancelled.", events: [] };
    },
  };
  const root = createRoot(document.getElementById("root"));
  const render = async pageId => {
    const panel = createElement(EvidencePanel, {
      adapter, sessionId: "session", pageId, pageUrl: `https://page${pageId}.example`,
      onRun: async (...args) => { results.push(args); },
    });
    await act(async () => { root.render(strict ? createElement(StrictMode, null, panel) : panel); });
  };
  const refresh = async () => { await act(async () => { document.querySelector(".evidence-panel > button").click(); }); };
  const propose = async () => {
    await act(async () => {
      document.querySelector(".evidence-actions").dispatchEvent(new window.Event("submit", { bubbles: true, cancelable: true }));
    });
  };
  try {
    await render(0);
    await check({ calls, results, render, refresh, propose, document });
  } finally {
    await act(async () => { root.unmount(); });
    dom.window.close();
    for (const [key, descriptor] of previous) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor);
      else delete globalThis[key];
    }
  }
}

test("a delayed observation from another tab cannot populate current evidence", async () => {
  const pending = deferred();
  await withPanel(async ({ render, refresh, document }) => {
    await refresh();
    await render(1);
    await act(async () => { pending.resolve(observed(0)); });
    assert.equal(document.querySelector(".evidence-source"), null);
    assert.equal(document.querySelector(".evidence-actions"), null);
  }, { observation: () => pending.promise });
});

test("switching away and back retires the original observation generation", async () => {
  const pending = deferred();
  await withPanel(async ({ render, refresh, document }) => {
    await refresh();
    await render(1);
    await render(0);
    await act(async () => { pending.resolve(observed(0, 1)); });
    assert.equal(document.querySelector(".evidence-source"), null, "old A response must stay retired across A → B → A");
    assert.equal(document.querySelector(".evidence-actions"), null);
  }, { observation: () => pending.promise });
});

for (const dispatchState of ["unknown", "acknowledged"]) {
  test(`a ${dispatchState} receipt after tab switch reaches run presentation`, async () => {
    const pending = deferred();
    const result = {
      status: "completed", events: [{ type: "ToolCallResult", receipt: { dispatch: dispatchState, verification: "unavailable", message: "Inspect the originating page." } }],
    };
    await withPanel(async ({ render, refresh, propose, results, calls, document }) => {
      await refresh();
      await propose();
      assert.equal(calls.find(call => call[0] === "dispatch")[2], 0);
      await render(1);
      await act(async () => { pending.resolve(result); });
      assert.equal(results.length, 1, "already-dispatched action receipt must not be lost on a scope change");
      assert.equal(results[0][0].events[0].receipt.dispatch, dispatchState);
      assert.deepEqual(results[0][1], { sessionId: "session", pageId: 0, pageUrl: "https://page0.example" }, "receipt must identify its originating tab");
      assert.equal(document.querySelector(".evidence-actions"), null, "source targets must not leak to new tab");
    }, { dispatch: () => pending.promise });
  });
}

test("a delayed prior-page approval is cancelled instead of resurfacing on another tab", async () => {
  const pending = deferred();
  await withPanel(async ({ render, refresh, propose, calls, results }) => {
    await refresh();
    await propose();
    await render(1);
    await act(async () => { pending.resolve({ run_id: "prior-run", status: "awaiting_approval", events: [{ type: "ApprovalRequested" }] }); });
    assert.deepEqual(calls.filter(call => call[0] === "cancel"), [["cancel", "prior-run"]]);
    assert.ok(results.every(([result]) => result.status !== "awaiting_approval"), "prior-page approval cannot become a current-page card");
    assert.ok(results.length > 0, "cancellation should be explained in run presentation");
    assert.deepEqual(results[0][1], { sessionId: "session", pageId: 0, pageUrl: "https://page0.example" });
  }, { dispatch: () => pending.promise });
});

test("a proposal uses the displayed document identity and retires targets afterward", async () => {
  await withPanel(async ({ refresh, propose, calls, results, document }) => {
    await refresh();
    assert.match(document.body.textContent, /Visual evidence unavailable/);
    assert.match(document.body.textContent, /Background network isolation unavailable/);
    await propose();
    const command = calls.find(call => call[0] === "dispatch")[3];
    assert.equal(command.name, "scroll_target");
    assert.equal(command.arguments.target_id, "target-0");
    assert.deepEqual(JSON.parse(command.arguments.document), observed(0).document);
    assert.equal(results.length, 1);
    assert.equal(document.querySelector(".evidence-actions"), null);
  });
});

test("effect cleanup replay does not disable evidence refresh", async () => {
  await withPanel(async ({ refresh, document }) => {
    await refresh();
    assert.match(document.querySelector(".evidence-source")?.textContent ?? "", /Reviewed page 0/);
  }, { strict: true });
});


test("a missing transport response after switching tabs preserves its source and uncertainty", async () => {
  const pending = deferred();
  await withPanel(async ({ render, refresh, propose, results }) => {
    await refresh();
    await propose();
    await render(1);
    await act(async () => { pending.reject(new Error("Host transport disconnected")); });
    assert.equal(results.length, 1);
    const [result, source] = results[0];
    assert.equal(result.status, "failed");
    assert.deepEqual(source, { sessionId: "session", pageId: 0, pageUrl: "https://page0.example" });
    assert.match(result.final_response, /response unavailable/i);
    assert.match(result.final_response, /inspect https:\/\/page0\.example before repeating/i);
    assert.match(result.final_response, /Host transport disconnected/);
    assert.ok(!(result.events || []).some(event => event.receipt?.dispatch === "not_dispatched"), "missing response cannot invent safe nondispatch");
  }, { dispatch: () => pending.promise });
});

test("a failed prior-page cancellation retains the pending run and explains the failure", async () => {
  const pending = deferred();
  const approval = {
    run_id: "still-pending-run", status: "awaiting_approval", events: [{ type: "ApprovalRequested", tool: "submit_target" }],
    approval_context: { url: "https://page0.example", target: { label: "Confirm route", destination: "https://page0.example/dispatch" } },
  };
  await withPanel(async ({ render, refresh, propose, results, calls }) => {
    await refresh();
    await propose();
    await render(1);
    await act(async () => { pending.resolve(approval); });
    assert.deepEqual(calls.filter(call => call[0] === "cancel"), [["cancel", approval.run_id]]);
    assert.equal(results.length, 1);
    const [result, source] = results[0];
    assert.equal(result.status, "awaiting_approval");
    assert.equal(result.run_id, approval.run_id);
    assert.deepEqual(result.approval_context, approval.approval_context);
    assert.match(result.final_response, /still pending/i);
    assert.match(result.final_response, /cancellation failed/i);
    assert.deepEqual(source, { sessionId: "session", pageId: 0, pageUrl: "https://page0.example" });
  }, { dispatch: () => pending.promise, cancel: async () => { throw new Error("Cancellation transport unavailable"); } });
});
