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

async function withApp(check, {
  closeFails = false,
  navigateFails = false,
  browserActionFails = false,
  snapshotFails = false,
} = {}) {
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
    navigate: async (session, page, url) => {
      calls.push(["navigate", session, page, url]);
      if (navigateFails) throw new Error("Host refused navigation");
    },
    browserAction: async (command, session, page) => {
      calls.push(["browser", command, session, page]);
      if (browserActionFails) throw new Error("Host refused browser action");
    },
    getPageSnapshot: async (session, page) => {
      calls.push(["snapshot", session, page]);
      if (snapshotFails) throw new Error("Browser runtime read timed out");
      return {
        url: `https://page${page}.example`,
        title: `Page ${page}`,
        link_count: page + 3,
      };
    },
  };
  const root = createRoot(document.getElementById("root"));
  const elementProto = dom.window.HTMLElement.prototype;
  if (typeof elementProto.attachEvent !== "function") {
    elementProto.attachEvent = () => {};
    elementProto.detachEvent = () => {};
  }
  const click = async (element) => {
    assert.ok(element, "expected a mounted control");
    await act(async () => { element.click(); });
  };
  // Controlled address-bar text commits on keyup. jsdom does not implement requestSubmit.
  const goTo = async (nextUrl) => {
    const input = document.querySelector(".url-input");
    const setter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, "value").set;
    await act(async () => {
      input.dispatchEvent(new window.Event("focusin", { bubbles: true }));
      const tracker = input._valueTracker;
      if (tracker) tracker.setValue("");
      setter.call(input, nextUrl);
      input.dispatchEvent(new window.Event("keyup", { bubbles: true }));
    });
    await act(async () => {
      document.querySelector(".url-bar").dispatchEvent(
        new window.Event("submit", { bubbles: true, cancelable: true })
      );
    });
  };
  const toolbar = async (label) => {
    const button = [...document.querySelectorAll(".browser-toolbar button")].find((item) => item.textContent === label);
    await click(button);
  };
  try {
    await act(async () => { root.render(createElement(App, { adapter, lane: "tauri" })); });
    await check({ calls, click, document, goTo, toolbar });
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

test("a failed navigation stays a navigation failure and does not read the page", async () => {
  await withApp(async ({ calls, document, goTo }) => {
    await goTo("https://example.test");
    assert.deepEqual(calls, [["navigate", "session", 0, "https://example.test"]]);
    assert.match(document.querySelector(".status-bar").textContent, /Navigation failed: Host refused navigation/);
    assert.equal(document.querySelector(".loading-overlay"), null);
  }, { navigateFails: true });
});

test("a failed snapshot read after navigation is a page refresh failure", async () => {
  await withApp(async ({ calls, document, goTo }) => {
    await goTo("https://example.test");
    assert.deepEqual(calls, [
      ["navigate", "session", 0, "https://example.test"],
      ["snapshot", "session", 0],
    ]);
    const status = document.querySelector(".status-bar").textContent;
    assert.match(status, /Page refresh failed: Browser runtime read timed out/);
    assert.doesNotMatch(status, /Navigation failed/);
    assert.equal(document.querySelector(".loading-overlay"), null);
  }, { snapshotFails: true });
});

test("back, forward, and reload keep an action failure distinct from a page read failure", async () => {
  await withApp(async ({ calls, document, toolbar }) => {
    await toolbar("Back");
    assert.deepEqual(calls, [["browser", "browser_back", "session", 0]]);
    assert.match(document.querySelector(".status-bar").textContent, /Browser action failed: Host refused browser action/);
  }, { browserActionFails: true });

  await withApp(async ({ calls, document, toolbar }) => {
    for (const [label, command] of [
      ["Back", "browser_back"],
      ["Forward", "browser_forward"],
      ["Reload", "browser_reload"],
    ]) {
      calls.length = 0;
      await toolbar(label);
      assert.deepEqual(calls, [
        ["browser", command, "session", 0],
        ["snapshot", "session", 0],
      ]);
    }
    const status = document.querySelector(".status-bar").textContent;
    assert.match(status, /Page refresh failed: Browser runtime read timed out/);
    assert.doesNotMatch(status, /Browser action failed/);
  }, { snapshotFails: true });
});
