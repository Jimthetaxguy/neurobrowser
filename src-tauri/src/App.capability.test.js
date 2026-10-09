import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { JSDOM } from "jsdom";
import { createServer } from "vite";

let server;
let App;
before(async () => {
  server = await createServer({ server: { middlewareMode: true, watch: null, hmr: false, ws: false }, appType: "custom" });
  ({ default: App } = await server.ssrLoadModule("/src/App.jsx"));
});
after(async () => { await server?.close(); });

test("the mounted approval card displays reviewed source, target and destination with redacted arguments", async () => {
  const dom = new JSDOM("<div id='root'></div>", { url: "https://shell.example" });
  const previous = new Map();
  for (const [key, value] of Object.entries({ window: dom.window, document: dom.window.document, IS_REACT_ACT_ENVIRONMENT: true })) {
    previous.set(key, Object.getOwnPropertyDescriptor(globalThis, key));
    Object.defineProperty(globalThis, key, { configurable: true, writable: true, value });
  }
  const source = "https://manifest.example/routes/north";
  const destination = "https://dispatch.example/confirm?route=north";
  const label = "Confirm North route & 17 crates";
  const documentStamp = { runtime_id: "page-runtime-0", document_id: "document-native-17", revision: 9 };
  const target = { id: "target-opaque-42", role: "button", tag: "button", label, disabled: false, sensitive: false, destination };
  const approval = {
    run_id: "reviewed-run", status: "awaiting_approval", final_response: "Review the scoped action before approval.",
    approval_context: { url: source, title: "North route manifest", document: documentStamp, target },
    events: [{ type: "ApprovalRequested", tool: "submit_target", decision: {
      reasons: ["Form submission requires approval"],
      redacted_arguments: { document: JSON.stringify(documentStamp), target_id: target.id, text: "[REDACTED]" },
    } }],
  };
  const commands = [];
  const adapter = {
    rendersPageInHost: false,
    createSession: async () => "session",
    createPage: async () => 0,
    getActionPolicy: async () => null,
    getPageSnapshot: async () => ({ url: source, title: "North route manifest" }),
    getPageObservation: async () => ({
      schema_version: 1, url: source, title: "North route manifest", text: "North route stock: 17 crates.",
      document: documentStamp, targets: [target], links: [], tables: [], omissions: [],
      capabilities: { javascript: true, interaction: true, scoped_targets: true, screenshots: false, enforcing_subresource_network: false },
    }),
    executeBrowserTool: async (...args) => { commands.push(args); return approval; },
  };
  const root = createRoot(document.getElementById("root"));
  try {
    await act(async () => { root.render(createElement(App, { adapter, lane: "tauri" })); });
    await act(async () => { document.querySelector(".evidence-panel > button").click(); });
    await act(async () => {
      document.querySelector(".evidence-actions").dispatchEvent(new window.Event("submit", { bubbles: true, cancelable: true }));
    });
    assert.equal(commands.length, 1);
    const card = document.querySelector(".approval-card");
    assert.ok(card, "governed result must reach the mounted application approval card");
    const context = card.querySelector(".approval-context");
    assert.ok(context, "approval needs context beyond opaque command IDs");
    assert.ok(context.textContent.includes(`Page: ${source}`));
    assert.ok(context.textContent.includes(label));
    assert.ok(context.textContent.includes(`Destination: ${destination}`));
    assert.match(context.textContent, /Reviewed revision: 9/);
    assert.match(card.querySelector(".approval-args").textContent, /\[REDACTED\]/);
    assert.deepEqual([...card.querySelectorAll("button")].map(button => button.textContent.trim()), ["Approve", "Deny", "Cancel"]);
  } finally {
    await act(async () => { root.unmount(); });
    dom.window.close();
    for (const [key, descriptor] of previous) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor);
      else delete globalThis[key];
    }
  }
});
