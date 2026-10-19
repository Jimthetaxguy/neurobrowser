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
