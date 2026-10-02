import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { JSDOM } from "jsdom";
import { createServer } from "vite";

let server;
let App;

before(async () => {
  server = await createServer({
    server: { middlewareMode: true, watch: null, hmr: false },
    appType: "custom",
  });
  ({ default: App } = await server.ssrLoadModule("/src/App.jsx"));
});

after(async () => {
  await server?.close();
});

async function withApp(check, { closeFails = false } = {}) {
  const dom = new JSDOM("<div id='root'></div>", { url: "https://shell.example" });
  const globals = {
    window: dom.window,
    document: dom.window.document,
    IS_REACT_ACT_ENVIRONMENT: true,
  };
  const previous = new Map();
  for (const [key, value] of Object.entries(globals)) {
    previous.set(key, Object.getOwnPropertyDescriptor(globalThis, key));
    Object.defineProperty(globalThis, key, { configurable: true, writable: true, value });
  }
  const calls = [];
  let nextPage = 0;
  const adapter = {
    rendersPageInHost: false,
    createSession: async () => "session",
    createPage: async () => nextPage++,
    getActionPolicy: async () => null,
    setActivePage: async (session, page) => { calls.push(["activate", session, page]); },
    closePage: async (session, page) => {
      calls.push(["close", session, page]);
      if (closeFails) throw new Error("Host refused close");
    },
    getPageSnapshot: async (session, page) => {
      calls.push(["snapshot", session, page]);
      return {
        url: `https://page${page}.example`,
        title: `Page ${page}`,
        link_count: page + 3,
      };
    },
  };
  const root = createRoot(document.getElementById("root"));
  const click = async (element) => {
    assert.ok(element, "expected a mounted control");
    await act(async () => { element.click(); });
  };
  try {
    await act(async () => { root.render(createElement(App, { adapter, lane: "tauri" })); });
    await check({ calls, click, document });
  } finally {
    await act(async () => { root.unmount(); });
    dom.window.close();
    for (const [key, descriptor] of previous) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor);
      else delete globalThis[key];
    }
  }
}

test("the last tab has no close control, and multiple tabs use sibling buttons", async () => {
  await withApp(async ({ click, document }) => {
    assert.equal(document.querySelectorAll(".tab").length, 1);
    assert.equal(document.querySelectorAll(".tab-close").length, 0);
    await click(document.querySelector(".header > button"));
    assert.equal(document.querySelectorAll(".tab").length, 2);
    for (const tab of document.querySelectorAll(".tab")) {
      const title = tab.querySelector(".tab-title");
      const close = tab.querySelector(".tab-close");
      assert.equal(title.tagName, "BUTTON");
      assert.equal(close.tagName, "BUTTON");
      assert.equal(title.parentElement, close.parentElement);
      assert.equal(close.closest("button"), close);
    }
  });
});

test("chip padding activates a tab, while title clicks activate it only once", async () => {
  await withApp(async ({ calls, click, document }) => {
    await click(document.querySelector(".header > button"));
    await click(document.querySelector(".tab"));
    assert.deepEqual(calls, [["activate", "session", 0], ["snapshot", "session", 0]]);
    assert.equal(document.querySelector(".tab.active .tab-title").textContent, "Page 0");
    calls.length = 0;
    await click(document.querySelectorAll(".tab-title")[1]);
    assert.deepEqual(calls, [["activate", "session", 1], ["snapshot", "session", 1]]);
  });
});

test("closing the active tab refreshes the survivor URL, title, and statistics", async () => {
  await withApp(async ({ calls, click, document }) => {
    await click(document.querySelector(".header > button"));
    await click(document.querySelector(".tab.active .tab-close"));
    assert.deepEqual(calls, [
      ["close", "session", 1],
      ["activate", "session", 0],
      ["snapshot", "session", 0],
    ]);
    assert.equal(document.querySelector(".url-input").value, "https://page0.example");
    assert.equal(document.querySelector(".tab-title").textContent, "Page 0");
    assert.equal(document.querySelector(".page-stats .stat-value").textContent, "3");
    assert.equal(document.querySelectorAll(".tab-close").length, 0);
  });
});

test("a failed host close keeps both tabs and reports the failure", async () => {
  await withApp(async ({ calls, click, document }) => {
    await click(document.querySelector(".header > button"));
    await click(document.querySelector(".tab.active .tab-close"));
    assert.deepEqual(calls, [["close", "session", 1]]);
    assert.equal(document.querySelectorAll(".tab").length, 2);
    assert.match(document.querySelector(".status-bar").textContent, /Close tab failed: Host refused close/);
  }, { closeFails: true });
});
