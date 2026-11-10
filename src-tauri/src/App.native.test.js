import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { JSDOM } from "jsdom";
import { createServer } from "vite";
let server, App;
before(async () => {
  server = await createServer({ server: { middlewareMode: true, watch: null, hmr: false, ws: false }, appType: "custom" });
  ({ default: App } = await server.ssrLoadModule("/src/App.jsx"));
});
after(async () => { await server?.close(); });
async function mounted(adapter, check) {
  const dom = new JSDOM("<div id='root'></div>", { url: "https://shell.example" });
  const previous = new Map();
  for (const [key, value] of Object.entries({window:dom.window, document:dom.window.document, IS_REACT_ACT_ENVIRONMENT:true})) {
    previous.set(key,Object.getOwnPropertyDescriptor(globalThis,key));
    Object.defineProperty(globalThis,key,{configurable:true,writable:true,value});
  }
  const root=createRoot(document.getElementById("root"));
  try {
    await act(async () => root.render(createElement(App,{adapter,lane:"tauri"})));
    await check(dom.window);
  } finally {
    await act(async () => root.unmount()); dom.window.close();
    for (const [key,descriptor] of previous) { if(descriptor)Object.defineProperty(globalThis,key,descriptor);else delete globalThis[key]; }
  }
}
test("missing native runtime never claims readiness or enables model requests", async () => {
  await mounted({createSession:async()=>{throw Error("IPC unavailable");}},async () => {
    assert.match(document.querySelector(".runtime-notice").textContent,/Open the native app/);
    assert.doesNotMatch(document.body.textContent,/runtime initialized/i);
    assert.equal(document.querySelector(".chat-input").disabled,true);
    assert.equal(document.querySelector(".chat-input-area .btn").disabled,true);
  });
});
test("native assistant shares no page by default, cancels, and ignores a late reply", async () => {
  let resolveReply;const calls=[];
  const adapter={createSession:async()=>"session",createPage:async()=>0,getActionPolicy:async()=>null,
    getProviderStatus:async()=>({provider:"openai",model:"native-model",configured:false}),
    askAssistant:async(...args)=>{calls.push(args);return new Promise(resolve=>{resolveReply=resolve;});},
    cancelAssistantRequest:async id=>{calls.push(["cancel",id]);}};
  await mounted(adapter,async win=>{
    assert.equal(document.querySelector(".settings-panel").hidden,true);
    await act(async()=>document.querySelector('[aria-label="Browser settings"]').click());
    assert.equal(document.querySelector(".settings-panel").hidden,false);
    assert.match(document.querySelector(".provider-status").textContent,/Setup needed/);
    await act(async()=>{
      const input=document.querySelector(".chat-input");
      Object.getOwnPropertyDescriptor(win.HTMLTextAreaElement.prototype,"value").set.call(input,"What is two plus two?");
      input.dispatchEvent(new win.Event("input",{bubbles:true}));
    });
    await act(async()=>document.querySelector(".chat-input-area").dispatchEvent(new win.Event("submit",{bubbles:true,cancelable:true})));
    assert.equal(calls.length,1);
    assert.equal(calls[0][4],false);
    assert.match(calls[0][3],/^[a-f0-9-]{36}$/);
    await act(async()=>document.querySelector(".chat-input-area .btn").click());
    assert.deepEqual(calls[1],["cancel",calls[0][3]]);
    await act(async()=>resolveReply("LATE_REPLY_CANARY"));
    assert.doesNotMatch(document.querySelector(".chat-messages").textContent,/LATE_REPLY_CANARY/);
    await act(async()=>document.querySelector('[aria-label="Toggle assistant"]').click());
    assert.equal(document.querySelector(".sidebar"),null);
    assert.equal(document.querySelector("main").className,"assistant-closed");
  });
});
test("approval is held while a provider request is in flight and no reply is dropped", async () => {
  const source = "https://manifest.example/";
  const documentStamp = { runtime_id: "page-runtime-0", document_id: "doc-1", revision: 1 };
  const target = { id: "t-1", role: "button", tag: "button", label: "Confirm", disabled: false, sensitive: false };
  const approval = {
    run_id: "run-1", status: "awaiting_approval", final_response: "Review first.",
    approval_context: { url: source, title: "Manifest", document: documentStamp, target },
    events: [{ type: "ApprovalRequested", tool: "click_target", decision: { reasons: ["Needs approval"], redacted_arguments: {} } }],
  };
  let resolveReply, resolveSubmit;
  const calls = [];
  const adapter = {
    rendersPageInHost: false,
    createSession: async () => "session", createPage: async () => 0, getActionPolicy: async () => null,
    getPageSnapshot: async () => ({ url: source, title: "Manifest" }),
    getPageObservation: async () => ({
      schema_version: 1, url: source, title: "Manifest", text: "Manifest.", document: documentStamp,
      targets: [target], links: [], tables: [], omissions: [],
      capabilities: { javascript: true, interaction: true, scoped_targets: true, screenshots: false, enforcing_subresource_network: false },
    }),
    executeBrowserTool: async () => approval,
    askAssistant: async (...args) => { calls.push(["ask", args[3]]); return new Promise(resolve => { resolveReply = resolve; }); },
    cancelAssistantRequest: async id => { calls.push(["cancel", id]); },
    submitApproval: async (runId, approved) => {
      calls.push(["submit", runId, approved]);
      return new Promise(resolve => { resolveSubmit = resolve; });
    },
  };
  await mounted(adapter, async win => {
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 0)); });
    await act(async () => { document.querySelector(".evidence-panel > button").click(); });
    await act(async () => {
      document.querySelector(".evidence-actions").dispatchEvent(new win.Event("submit", { bubbles: true, cancelable: true }));
    });
    const approveButton = () => [...document.querySelectorAll(".approval-card button")].find(b => b.textContent.trim() === "Approve");
    assert.ok(approveButton(), "approval card is shown");
    assert.equal(approveButton().disabled, false);

    // Ask while the approval is pending.
    await act(async () => {
      const input = document.querySelector(".chat-input");
      Object.getOwnPropertyDescriptor(win.HTMLTextAreaElement.prototype, "value").set.call(input, "First question");
      input.dispatchEvent(new win.Event("input", { bubbles: true }));
    });
    await act(async () => document.querySelector(".chat-input-area").dispatchEvent(new win.Event("submit", { bubbles: true, cancelable: true })));
    assert.equal(calls.filter(c => c[0] === "ask").length, 1);

    // Approve is blocked while the provider request is in flight.
    assert.equal(approveButton().disabled, true);
    await act(async () => approveButton().click());
    assert.equal(calls.some(c => c[0] === "submit"), false);
    assert.equal(document.querySelector(".chat-input").disabled, true);

    await act(async () => resolveReply("FIRST_REPLY_CANARY"));
    assert.match(document.querySelector(".chat-messages").textContent, /FIRST_REPLY_CANARY/);

    // The held approval is now actionable. Resolving blocks a new ask and a second approve.
    assert.equal(approveButton().disabled, false);
    await act(async () => approveButton().click());
    await act(async () => approveButton()?.click());
    assert.deepEqual(calls.filter(c => c[0] === "submit"), [["submit", "run-1", true]]);
    assert.equal(document.querySelector(".chat-input").disabled, true);

    await act(async () => resolveSubmit({ run_id: "run-1", status: "completed", final_response: "Done.", events: [] }));
    assert.equal(document.querySelector(".approval-card"), null);
    assert.equal(document.querySelector(".chat-input").disabled, false);
  });
});
