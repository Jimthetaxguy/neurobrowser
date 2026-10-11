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
    server: { middlewareMode: true, watch: null, hmr: false, ws: false },
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
  reloadUnconfirmed = false,
  lane = "tauri",
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
  const hostListeners = new Set();
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
      if (command === "browser_reload" && reloadUnconfirmed) {
        throw new Error(
          "Reload dispatched; page readiness is unconfirmed: Timed out waiting for page 0 to finish loading. Do not repeat the reload automatically; inspect the page."
        );
      }
      if (browserActionFails) throw new Error("Host refused browser action");
    },
    getPageSnapshot: async (session, page) => {
      calls.push(["snapshot", session, page]);
      if (snapshotFails) throw new Error("Browser runtime read timed out");
      if (lane === "appkit") {
        return { url: "https://old.example", title: "Old title", link_count: 9 };
      }
      return {
        url: `https://page${page}.example`,
        title: `Page ${page}`,
        link_count: page + 3,
      };
    },
    ...(lane === "appkit" ? {
      onHostEvent(callback) {
        hostListeners.add(callback);
        return () => hostListeners.delete(callback);
      },
    } : {}),
  };
  const dispatch = async (event) => {
    await act(async () => {
      for (const listener of hostListeners) listener(event);
    });
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
    await act(async () => { root.render(createElement(App, { adapter, lane })); });
    await check({ calls, click, dispatch, document, goTo, toolbar });
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

const statusText = (document) => document.querySelector(".status-bar").textContent;

test("AppKit panel navigation stays in flight until a newer native load reports", async () => {
  await withApp(async ({ calls, dispatch, document, goTo, toolbar }) => {
    await dispatch({ type: "navigation", pageId: 0, navigationId: 1, url: "https://old.example/" });
    await dispatch({
      type: "snapshot", pageId: 0, navigationId: 1, reason: "finish", loading: false,
      snapshot: { url: "https://old.example/", title: "Old title" },
    });
    assert.match(statusText(document), /Loaded: Old title/);

    await goTo("https://next.example");
    assert.equal(calls.some((call) => call[0] === "snapshot"), false);
    assert.match(statusText(document), /Navigating to https:\/\/next\.example/);
    // A late report from the load that was already finished does not settle the panel action.
    await dispatch({
      type: "snapshot", pageId: 0, navigationId: 1, reason: "finish", loading: false,
      snapshot: { url: "https://old.example/", title: "Old title" },
    });
    assert.match(statusText(document), /Navigating to https:\/\/next\.example/);
    assert.equal(document.querySelector(".url-input").value, "https://next.example");

    await dispatch({ type: "navigation", pageId: 0, navigationId: 2, url: "https://next.example/" });
    assert.match(statusText(document), /Loading https:\/\/next\.example\/…/);
    await dispatch({
      type: "snapshot", pageId: 0, navigationId: 2, reason: "finish", loading: false,
      snapshot: { url: "https://next.example/", title: "Next" },
    });
    assert.match(statusText(document), /Loaded: Next/);
    assert.equal(document.querySelector(".url-input").value, "https://next.example/");

    await toolbar("Back");
    assert.match(statusText(document), /Going back…/);
    await dispatch({
      type: "snapshot", pageId: 0, navigationId: 1, reason: "finish", loading: false,
      snapshot: { url: "https://old.example/", title: "Old title" },
    });
    assert.match(statusText(document), /Going back…/);
    assert.equal(document.querySelector(".url-input").value, "https://next.example/");
  }, { lane: "appkit" });
});

test("AppKit native loads update the panel after a rejected URL", async () => {
  await withApp(async ({ dispatch, document, goTo }) => {
    await dispatch({ type: "navigation", pageId: 0, navigationId: 1, url: "https://old.example/" });
    await dispatch({
      type: "snapshot", pageId: 0, navigationId: 1, reason: "finish", loading: false,
      snapshot: { url: "https://old.example/", title: "Old title" },
    });
    await goTo("ftp://files.example");
    await dispatch({
      type: "status", pageId: 0, navigationId: 1, message: "Only http and https URLs can be opened",
    });
    await dispatch({
      type: "snapshot", pageId: 0, navigationId: 1, reason: "finish", loading: false,
      snapshot: { url: "https://old.example/", title: "Old title" },
    });
    assert.match(statusText(document), /Only http and https URLs can be opened/);

    // Native address bar, back/forward, or reload: a load the panel did not start.
    await dispatch({ type: "navigation", pageId: 0, navigationId: 2, url: "https://native.example/" });
    assert.match(statusText(document), /Loading https:\/\/native\.example\/…/);
    assert.equal(document.querySelector(".url-input").value, "https://native.example/");
    await dispatch({
      type: "snapshot", pageId: 0, navigationId: 2, reason: "finish", loading: false,
      snapshot: { url: "https://native.example/", title: "Native" },
    });
    assert.match(statusText(document), /Loaded: Native/);

    // A rejection on the native side, then a native tab selection of the same page.
    await dispatch({
      type: "status", pageId: 0, navigationId: 2, message: "Only http and https URLs can be opened",
    });
    await dispatch({
      type: "snapshot", pageId: 0, navigationId: 2, reason: "select", loading: false,
      snapshot: { url: "https://native.example/", title: "Native" },
    });
    assert.match(statusText(document), /Loaded: Native/);
  }, { lane: "appkit" });
});

test("AppKit tab selection during a slow load does not report the old page as loaded", async () => {
  await withApp(async ({ click, dispatch, document }) => {
    await click(document.querySelector(".header > button"));
    await click(document.querySelector(".tab"));
    await dispatch({ type: "navigation", pageId: 0, navigationId: 1, url: "https://first.example/" });
    await dispatch({
      type: "snapshot", pageId: 0, navigationId: 1, reason: "finish", loading: false,
      snapshot: { url: "https://first.example/", title: "First" },
    });
    assert.match(statusText(document), /Loaded: First/);

    await dispatch({ type: "navigation", pageId: 0, navigationId: 2, url: "https://slow.example/" });
    // Re-selecting the active tab (or switching away and back) while it loads.
    await dispatch({
      type: "snapshot", pageId: 0, navigationId: 2, reason: "select", loading: true,
      snapshot: { url: "https://first.example/", title: "First" },
    });
    assert.match(statusText(document), /Loading/);
    assert.doesNotMatch(statusText(document), /Loaded: First/);
    // A selection snapshot taken before the newer load started is older and is dropped.
    await dispatch({
      type: "snapshot", pageId: 0, navigationId: 1, reason: "select", loading: false,
      snapshot: { url: "https://first.example/", title: "First" },
    });
    assert.doesNotMatch(statusText(document), /Loaded: First/);
    // A background tab's finished load does not touch the active tab's status.
    await dispatch({ type: "navigation", pageId: 1, navigationId: 3, url: "https://background.example/" });
    await dispatch({
      type: "snapshot", pageId: 1, navigationId: 3, reason: "finish", loading: false,
      snapshot: { url: "https://background.example/", title: "Background" },
    });
    await dispatch({ type: "status", pageId: 1, navigationId: 3, message: "Background failed" });
    assert.match(statusText(document), /Loading/);
    assert.equal(document.querySelector(".url-input").value, "https://slow.example/");

    await dispatch({
      type: "snapshot", pageId: 0, navigationId: 2, reason: "finish", loading: false,
      snapshot: { url: "https://slow.example/", title: "Slow" },
    });
    assert.match(statusText(document), /Loaded: Slow/);
  }, { lane: "appkit" });
});

test("a failed snapshot after a tab switch or close reports page refresh failure", async () => {
  await withApp(async ({ click, document }) => {
    await click(document.querySelector(".header > button"));
    await click(document.querySelector(".tab"));
    assert.match(document.querySelector(".status-bar").textContent, /Page refresh failed: Browser runtime read timed out/);
    assert.equal(document.querySelector(".tab.active .tab-title").textContent, "Tab 1");
    assert.equal(document.querySelectorAll(".tab").length, 2);

    await click(document.querySelector(".tab.active .tab-close"));
    assert.match(document.querySelector(".status-bar").textContent, /Page refresh failed: Browser runtime read timed out/);
    assert.equal(document.querySelectorAll(".tab").length, 1);
    assert.equal(document.querySelector(".tab-title").textContent, "Tab 2");
  }, { snapshotFails: true });
});

test("an unconfirmed reload keeps its own status without the browser-action failure prefix", async () => {
  await withApp(async ({ calls, document, toolbar }) => {
    await toolbar("Reload");
    assert.deepEqual(calls, [["browser", "browser_reload", "session", 0]]);
    const status = document.querySelector(".status-bar").textContent;
    assert.match(status, /Reload dispatched; page readiness is unconfirmed/);
    assert.doesNotMatch(status, /Browser action failed/);
  }, { reloadUnconfirmed: true });
});
