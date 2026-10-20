import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { after, before, test } from "node:test";
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { JSDOM } from "jsdom";
import { createServer } from "vite";

const browserStyles = readFileSync(new URL("./styles.css", import.meta.url), "utf8");

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
  navigateError = "Host refused navigation",
  browserActionFails = false,
  snapshotFails = false,
  reloadUnconfirmed = false,
  lane = "tauri",
  history = null,
  keepStartFocus = false,
  memory = null,
  findInPage = null,
  adjustPageZoom = null,
} = {}) {
  const dom = new JSDOM("<div id='root'></div>", { url: "https://shell.example" });
  for (const name of ["Element", "HTMLElement", "HTMLInputElement"]) {
    const proto = dom.window[name]?.prototype;
    if (!proto || typeof proto.attachEvent === "function") continue;
    proto.attachEvent = () => {};
    proto.detachEvent = () => {};
  }
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
  const lifecycleListeners = new Set();
  const menuListeners = new Set();
  const newWindowListeners = new Set();
  const downloadListeners = new Set();
  const createdPages = [];
  const adapter = {
    rendersPageInHost: false,
    createSession: async () => "session",
    createPage: async (_session, options) => {
      createdPages.push(options ?? null);
      return nextPage++;
    },
    getActionPolicy: async () => null,
    setActivePage: async (session, page) => { calls.push(["activate", session, page]); },
    closePage: async (session, page) => {
      calls.push(["close", session, page]);
      if (closeFails) throw new Error("Host refused close");
    },
    navigate: async (session, page, url) => {
      calls.push(["navigate", session, page, url]);
      if (navigateFails) throw new Error(navigateError);
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
    onPageLifecycle(callback) {
      lifecycleListeners.add(callback);
      return () => lifecycleListeners.delete(callback);
    },
    onPageNewWindow(callback) {
      newWindowListeners.add(callback);
      return () => newWindowListeners.delete(callback);
    },
    onDownload(callback) {
      downloadListeners.add(callback);
      return () => downloadListeners.delete(callback);
    },
    openDownload: async (id) => { calls.push(["open-download", id]); },
    showDownloadInFolder: async (id) => { calls.push(["show-download", id]); },
    ...(typeof findInPage === "function" ? { findInPage } : {}),
    ...(typeof adjustPageZoom === "function" ? { adjustPageZoom } : {}),
    ...(history == null ? {} : {
      searchBrowsingHistory: async (query, limit) => {
        calls.push(["history", query, limit]);
        return typeof history === "function" ? history(query, limit) : history;
      },
    }),
    ...(memory == null ? {} : {
      searchLocalMemory: async (query, limit) => {
        calls.push(["memory", query, limit]);
        return typeof memory.search === "function" ? memory.search(query, limit) : memory.search;
      },
      explainMemoryResult: async (query, result, limit) => {
        calls.push(["explain", query, result?.block_id, limit]);
        return typeof memory.explain === "function" ? memory.explain(query, result, limit) : memory.explain;
      },
      forgetMemory: async (pageUrl) => {
        calls.push(["forget", pageUrl]);
        if (typeof memory.forget === "function") await memory.forget(pageUrl);
      },
    }),
    ...(lane === "appkit" ? {
      onHostEvent(callback) {
        hostListeners.add(callback);
        return () => hostListeners.delete(callback);
      },
    } : {
      onMenuCommand(callback) {
        menuListeners.add(callback);
        return () => menuListeners.delete(callback);
      },
    }),
  };
  const dispatch = async (event) => {
    await act(async () => {
      for (const listener of hostListeners) listener(event);
    });
  };
  const dispatchLifecycle = async (event) => {
    await act(async () => {
      for (const listener of lifecycleListeners) listener(event);
    });
  };
  const dispatchDownload = async (event) => {
    await act(async () => {
      for (const listener of downloadListeners) listener(event);
    });
  };
  const dispatchNewWindow = async (event) => {
    await act(async () => {
      for (const listener of newWindowListeners) listener(event);
    });
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 20));
    });
  };
  const dispatchMenu = async (command) => {
    await act(async () => {
      for (const listener of menuListeners) listener(command);
    });
  };
  const root = createRoot(document.getElementById("root"));
  const click = async (element) => {
    assert.ok(element, "expected a mounted control");
    await act(async () => { element.click(); });
  };
  // Controlled address-bar text commits on keyup. jsdom does not implement requestSubmit.
  const goTo = async (nextUrl, { submit = true } = {}) => {
    const input = document.querySelector(".url-input");
    const setter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, "value").set;
    await act(async () => {
      input.dispatchEvent(new window.Event("focusin", { bubbles: true }));
      const tracker = input._valueTracker;
      if (tracker) tracker.setValue("");
      setter.call(input, nextUrl);
      input.dispatchEvent(new window.Event("keyup", { bubbles: true }));
    });
    if (!submit) return;
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
  const keydown = async (key, { alt = false, code = "", ctrl = false, meta = false, repeat = false, shift = false, target } = {}) => {
    const event = new window.KeyboardEvent("keydown", {
      altKey: alt,
      bubbles: true,
      cancelable: true,
      code,
      ctrlKey: ctrl,
      key,
      metaKey: meta,
      repeat,
      shiftKey: shift,
    });
    await act(async () => {
      (target || document).dispatchEvent(event);
    });
    return event;
  };
  try {
    await act(async () => { root.render(createElement(App, { adapter, lane })); });
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 0));
    });
    if (!keepStartFocus) {
      await act(async () => {
        document.querySelector(".location .url-input")?.blur();
      });
    }
    await check({
      calls, click, createdPages, dispatch, dispatchDownload, dispatchLifecycle, dispatchMenu, dispatchNewWindow, document, goTo, keydown, toolbar,
    });
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

async function openMoreTabs(click, document, count) {
  for (let index = 0; index < count; index += 1) {
    await click(document.querySelector(".header > button"));
  }
  await act(async () => {
    document.querySelector(".location .url-input")?.blur();
  });
}

test("Cmd/Ctrl-R reloads the active tab", async () => {
  await withApp(async ({ calls, document, keydown }) => {
    for (const options of [{ ctrl: true }, { meta: true }]) {
      calls.length = 0;
      const event = await keydown("r", options);
      assert.equal(event.defaultPrevented, true);
      assert.deepEqual(calls, [
        ["browser", "browser_reload", "session", 0],
        ["snapshot", "session", 0],
      ]);
      assert.match(document.querySelector(".status-bar").textContent, /Loaded: Page 0/);
    }
    for (const options of [
      { ctrl: true, shift: true },
      { meta: true, alt: true },
      { ctrl: true, repeat: true },
      {},
    ]) {
      calls.length = 0;
      const event = await keydown("r", options);
      assert.equal(event.defaultPrevented, false);
      assert.deepEqual(calls, []);
    }
  });

  await withApp(async ({ calls, document, keydown }) => {
    await keydown("r", { meta: true });
    assert.deepEqual(calls, [["browser", "browser_reload", "session", 0]]);
    assert.match(document.querySelector(".status-bar").textContent, /Browser action failed: Host refused browser action/);
  }, { browserActionFails: true });

  await withApp(async ({ calls, document, keydown }) => {
    await keydown("R", { ctrl: true });
    assert.deepEqual(calls, [["browser", "browser_reload", "session", 0]]);
    const status = document.querySelector(".status-bar").textContent;
    assert.match(status, /Reload dispatched; page readiness is unconfirmed/);
    assert.doesNotMatch(status, /Browser action failed/);
  }, { reloadUnconfirmed: true });
});

test("Cmd/Ctrl-[ goes back and Cmd/Ctrl-] goes forward", async () => {
  await withApp(async ({ calls, keydown }) => {
    const cases = [
      ["[", { ctrl: true }, "browser_back"],
      ["[", { meta: true }, "browser_back"],
      ["]", { ctrl: true }, "browser_forward"],
      ["]", { meta: true }, "browser_forward"],
      ["Unidentified", { code: "BracketLeft", meta: true }, "browser_back"],
      ["Unidentified", { code: "BracketRight", ctrl: true }, "browser_forward"],
    ];
    for (const [key, options, command] of cases) {
      calls.length = 0;
      const event = await keydown(key, options);
      assert.equal(event.defaultPrevented, true);
      assert.deepEqual(calls, [
        ["browser", command, "session", 0],
        ["snapshot", "session", 0],
      ]);
    }
    for (const [key, options] of [
      ["[", { ctrl: true, alt: true }],
      ["[", { meta: true, repeat: true }],
      ["]", {}],
      ["]", { ctrl: true, shift: true }],
    ]) {
      calls.length = 0;
      await keydown(key, options);
      assert.deepEqual(calls, []);
    }
  });
});

test("Cmd/Ctrl-1 through 8 select that tab and 9 selects the last", async () => {
  await withApp(async ({ calls, click, document, keydown }) => {
    await openMoreTabs(click, document, 9);
    for (let number = 1; number <= 8; number += 1) {
      calls.length = 0;
      const options = number % 2 === 0 ? { meta: true } : { ctrl: true };
      await keydown(String(number), options);
      assert.deepEqual(calls, [
        ["activate", "session", number - 1],
        ["snapshot", "session", number - 1],
      ]);
      assert.equal(document.querySelector(".tab.active .tab-title").textContent, `Page ${number - 1}`);
    }
    calls.length = 0;
    await keydown("9", { ctrl: true });
    assert.deepEqual(calls, [
      ["activate", "session", 9],
      ["snapshot", "session", 9],
    ]);
    calls.length = 0;
    await keydown("8", { meta: true });
    assert.deepEqual(calls, [
      ["activate", "session", 7],
      ["snapshot", "session", 7],
    ]);
    calls.length = 0;
    const sameTab = await keydown("8", { ctrl: true });
    assert.equal(sameTab.defaultPrevented, true);
    assert.deepEqual(calls, []);
  });

  await withApp(async ({ calls, click, document, keydown }) => {
    await openMoreTabs(click, document, 2);
    for (const key of ["4", "8"]) {
      calls.length = 0;
      const event = await keydown(key, { meta: true });
      assert.equal(event.defaultPrevented, true);
      assert.deepEqual(calls, []);
      assert.equal(document.querySelector(".tab.active .tab-title").textContent, "Tab 3");
    }
    await keydown("1", {});
    assert.equal(document.querySelector(".tab.active .tab-title").textContent, "Tab 3");
    await keydown("1", { meta: true });
    calls.length = 0;
    await keydown("9", { ctrl: true });
    assert.deepEqual(calls, [
      ["activate", "session", 2],
      ["snapshot", "session", 2],
    ]);
    assert.equal(document.querySelector(".tab.active .tab-title").textContent, "Page 2");
  });
});

test("Cmd/Ctrl-Shift-[ and Shift-] select the previous and next tab", async () => {
  await withApp(async ({ calls, click, document, keydown }) => {
    await keydown("[", { ctrl: true, shift: true });
    await keydown("]", { meta: true, shift: true });
    assert.deepEqual(calls, []);

    await openMoreTabs(click, document, 2);
    const expectTab = async (key, options, pageId) => {
      calls.length = 0;
      const event = await keydown(key, options);
      assert.equal(event.defaultPrevented, true);
      assert.deepEqual(calls, [
        ["activate", "session", pageId],
        ["snapshot", "session", pageId],
      ]);
      assert.equal(document.querySelector(".tab.active .tab-title").textContent, `Page ${pageId}`);
    };
    await expectTab("[", { ctrl: true, shift: true }, 1);
    await expectTab("{", { meta: true }, 0);
    await expectTab("[", { code: "BracketLeft", meta: true, shift: true }, 2);
    await expectTab("]", { ctrl: true, shift: true }, 0);
    await expectTab("}", { meta: true }, 1);
    await expectTab("]", { code: "BracketRight", ctrl: true, shift: true }, 2);
    await expectTab("Unidentified", { code: "BracketLeft", ctrl: true, shift: true }, 1);
  });
});

test("text entry keeps history and tab chords quiet, while L, T, W, and R still run", async () => {
  await withApp(async ({ calls, click, document, keydown }) => {
    await openMoreTabs(click, document, 2);
    const urlInput = document.querySelector(".url-input");
    const chat = document.querySelector(".chat-input");
    const chords = [
      ["[", { ctrl: true }],
      ["]", { meta: true }],
      ["1", { ctrl: true }],
      ["9", { meta: true }],
      ["[", { ctrl: true, shift: true }],
      ["]", { meta: true, shift: true }],
      ["{", { ctrl: true }],
      ["}", { meta: true }],
    ];
    const focus = async (element) => {
      await act(async () => { element.focus(); });
    };
    const expectQuiet = async () => {
      for (const [key, options] of chords) {
        calls.length = 0;
        const event = await keydown(key, options);
        assert.equal(event.defaultPrevented, false);
        assert.deepEqual(calls, []);
      }
    };

    await focus(urlInput);
    await expectQuiet();
    await focus(chat);
    await expectQuiet();

    const field = document.createElement("input");
    document.body.appendChild(field);
    for (const type of ["search", "email", "password", "tel", "number", "text"]) {
      field.type = type;
      await focus(field);
      await expectQuiet();
    }
    const editable = document.createElement("div");
    editable.setAttribute("contenteditable", "true");
    editable.tabIndex = 0;
    document.body.appendChild(editable);
    await focus(editable);
    await expectQuiet();

    field.type = "checkbox";
    await focus(field);
    calls.length = 0;
    await keydown("[", { ctrl: true });
    assert.deepEqual(calls, [
      ["browser", "browser_back", "session", 2],
      ["snapshot", "session", 2],
    ]);
    field.remove();
    editable.remove();

    await focus(urlInput);
    calls.length = 0;
    const reload = await keydown("r", { ctrl: true });
    assert.equal(reload.defaultPrevented, true);
    assert.deepEqual(calls, [
      ["browser", "browser_reload", "session", 2],
      ["snapshot", "session", 2],
    ]);

    await focus(chat);
    calls.length = 0;
    await keydown("R", { meta: true });
    assert.deepEqual(calls, [
      ["browser", "browser_reload", "session", 2],
      ["snapshot", "session", 2],
    ]);

    const tabCount = document.querySelectorAll(".tab").length;
    await keydown("t", { ctrl: true });
    assert.equal(document.querySelectorAll(".tab").length, tabCount + 1);
    await keydown("l", { meta: true });
    assert.equal(document.activeElement, urlInput);
    await focus(urlInput);
    await keydown("w", { ctrl: true });
    assert.equal(document.querySelectorAll(".tab").length, tabCount);
  });
});

test("history and tab chords still run from a button or select", async () => {
  await withApp(async ({ calls, click, document, keydown }) => {
    await openMoreTabs(click, document, 1);
    const back = [...document.querySelectorAll(".browser-toolbar button")].find((item) => item.textContent === "Back");
    calls.length = 0;
    await keydown("[", { meta: true, target: back });
    assert.deepEqual(calls, [
      ["browser", "browser_back", "session", 1],
      ["snapshot", "session", 1],
    ]);
    calls.length = 0;
    await keydown("1", { ctrl: true, target: document.querySelector(".provider-select") });
    assert.deepEqual(calls, [
      ["activate", "session", 0],
      ["snapshot", "session", 0],
    ]);
    assert.equal(document.querySelector(".tab.active .tab-title").textContent, "Page 0");
  });
});

test("a netguard refusal is shown on the status line", async () => {
  const reason = "Refusing to fetch internal/loopback address (host '127.0.0.1')";
  await withApp(async ({ calls, document, goTo }) => {
    await goTo("http://127.0.0.1/");
    assert.deepEqual(calls, [["navigate", "session", 0, "http://127.0.0.1/"]]);
    assert.match(document.querySelector(".status-line").textContent, /Navigation failed: Refusing to fetch internal\/loopback address \(host '127\.0\.0\.1'\)/);
    assert.equal(document.querySelector(".load-progress"), null);
  }, { navigateError: reason, navigateFails: true });
});

test("page lifecycle updates the address bar, tab title, security indicator, and progress bar", async () => {
  await withApp(async ({ dispatchLifecycle, document }) => {
    assert.equal(document.querySelector(".load-progress"), null);
    assert.equal(document.querySelector("[data-security]"), null);

    await dispatchLifecycle({
      pageId: 0,
      phase: "started",
      url: "http://inpage.example/next",
      title: "",
    });
    assert.equal(document.querySelector(".url-input").value, "http://inpage.example/next");
    assert.equal(document.querySelector("[data-security='insecure']").textContent.trim(), "Not secure");
    assert.ok(document.querySelector(".load-progress"));
    assert.equal(document.querySelector(".tab-title").textContent, "Tab 1");

    await dispatchLifecycle({
      pageId: 0,
      phase: "title",
      url: "http://inpage.example/next",
      title: "In-page title",
    });
    assert.equal(document.querySelector(".tab-title").textContent, "In-page title");
    assert.equal(document.querySelector(".url-input").value, "http://inpage.example/next");

    await dispatchLifecycle({
      pageId: 0,
      phase: "finished",
      url: "https://inpage.example/landed",
      title: "",
    });
    assert.equal(document.querySelector(".load-progress"), null);
    assert.equal(document.querySelector(".url-input").value, "https://inpage.example/landed");
    assert.equal(document.querySelector(".tab-title").textContent, "In-page title");
    assert.ok(document.querySelector("[data-security='secure'] svg"));
    assert.equal(document.querySelector("[data-security='insecure']"), null);
  });
});

function reloadOrStop(document) {
  return [...document.querySelectorAll(".browser-toolbar button")].find((item) =>
    item.textContent === "Reload" || item.textContent === "Stop"
  );
}

test("the reload button is Stop while the page is loading", async () => {
  await withApp(async ({ calls, dispatchLifecycle, document, toolbar }) => {
    assert.equal(reloadOrStop(document).textContent, "Reload");
    assert.equal(document.querySelector(".load-progress"), null);

    await dispatchLifecycle({
      pageId: 0,
      phase: "started",
      url: "https://slow.example/",
      title: "",
    });
    assert.equal(reloadOrStop(document).textContent, "Stop");
    assert.ok(document.querySelector(".load-progress"));

    calls.length = 0;
    await toolbar("Stop");
    assert.deepEqual(calls, [["browser", "stop_page_load", "session", 0]]);
    assert.equal(reloadOrStop(document).textContent, "Reload");
    assert.equal(document.querySelector(".load-progress"), null);

    await dispatchLifecycle({
      pageId: 1,
      phase: "started",
      url: "https://other.example/",
      title: "",
    });
    assert.equal(reloadOrStop(document).textContent, "Reload");
    assert.equal(document.querySelector(".load-progress"), null);

    await dispatchLifecycle({
      pageId: 0,
      phase: "started",
      url: "https://slow.example/again",
      title: "",
    });
    assert.equal(reloadOrStop(document).textContent, "Stop");
    calls.length = 0;
    await dispatchLifecycle({
      pageId: 0,
      phase: "finished",
      url: "https://slow.example/again",
      title: "",
    });
    assert.equal(reloadOrStop(document).textContent, "Reload");
    assert.equal(document.querySelector(".load-progress"), null);
    assert.deepEqual(calls, []);

    calls.length = 0;
    await toolbar("Reload");
    assert.deepEqual(calls, [
      ["browser", "browser_reload", "session", 0],
      ["snapshot", "session", 0],
    ]);
  });
});

test("Escape stops a load from the address bar or the page unless that text is being edited", async () => {
  await withApp(async ({ calls, dispatchLifecycle, document, keydown }) => {
    calls.length = 0;
    const idleEsc = await keydown("Escape");
    assert.equal(idleEsc.defaultPrevented, false);
    assert.deepEqual(calls, []);
    assert.equal(reloadOrStop(document).textContent, "Reload");

    await dispatchLifecycle({
      pageId: 0,
      phase: "started",
      url: "https://slow.example/",
      title: "",
    });
    calls.length = 0;
    const pageEsc = await keydown("Escape");
    assert.equal(pageEsc.defaultPrevented, true);
    assert.deepEqual(calls, [["browser", "stop_page_load", "session", 0]]);
    assert.equal(reloadOrStop(document).textContent, "Reload");

    await dispatchLifecycle({
      pageId: 0,
      phase: "started",
      url: "https://slow.example/2",
      title: "",
    });
    const input = document.querySelector(".url-input");
    calls.length = 0;
    await act(async () => { input.focus(); });
    const addressEsc = await keydown("Escape");
    assert.equal(addressEsc.defaultPrevented, true);
    assert.equal(document.activeElement, input);
    assert.deepEqual(calls, [["browser", "stop_page_load", "session", 0]]);
    assert.equal(reloadOrStop(document).textContent, "Reload");

    await dispatchLifecycle({
      pageId: 0,
      phase: "started",
      url: "https://slow.example/3",
      title: "",
    });
    const setter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, "value").set;
    await act(async () => {
      input.focus();
      const tracker = input._valueTracker;
      if (tracker) tracker.setValue("");
      setter.call(input, "https://draft.example/editing");
      input.dispatchEvent(new window.Event("keyup", { bubbles: true }));
    });
    calls.length = 0;
    const editingEsc = await keydown("Escape");
    assert.equal(editingEsc.defaultPrevented, false);
    assert.equal(document.activeElement, input);
    assert.deepEqual(calls, []);
    assert.equal(input.value, "https://draft.example/editing");
    assert.equal(reloadOrStop(document).textContent, "Stop");

    const chat = document.querySelector(".chat-input");
    await act(async () => {
      input.blur();
      chat.focus();
    });
    calls.length = 0;
    const chatEsc = await keydown("Escape");
    assert.equal(chatEsc.defaultPrevented, false);
    assert.equal(document.activeElement, chat);
    assert.deepEqual(calls, []);
    assert.equal(reloadOrStop(document).textContent, "Stop");

    await act(async () => { chat.blur(); });
    calls.length = 0;
    const afterEdit = await keydown("Escape");
    assert.equal(afterEdit.defaultPrevented, true);
    assert.deepEqual(calls, [["browser", "stop_page_load", "session", 0]]);
    assert.equal(reloadOrStop(document).textContent, "Reload");
  });
});

test("AppKit keeps Reload and does not stop on Escape", async () => {
  await withApp(async ({ calls, dispatchLifecycle, document, keydown }) => {
    await dispatchLifecycle({
      pageId: 0,
      phase: "started",
      url: "https://slow.example/",
      title: "",
    });
    assert.equal(reloadOrStop(document).textContent, "Reload");
    calls.length = 0;
    const event = await keydown("Escape");
    assert.equal(event.defaultPrevented, false);
    assert.deepEqual(calls, []);
  }, { lane: "appkit" });
});

test("an address draft survives lifecycle and a background tab keeps its own title", async () => {
  await withApp(async ({ click, dispatchLifecycle, document }) => {
    await click(document.querySelector(".header > button"));
    await click(document.querySelector(".tab"));
    const input = document.querySelector(".url-input");
    const setter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, "value").set;
    await act(async () => {
      input.focus();
      const tracker = input._valueTracker;
      if (tracker) tracker.setValue("");
      setter.call(input, "https://draft.example/keep");
      input.dispatchEvent(new window.Event("keyup", { bubbles: true }));
    });
    assert.equal(input.value, "https://draft.example/keep");

    await dispatchLifecycle({
      pageId: 0,
      phase: "started",
      url: "http://replaced.example/",
      title: "",
    });
    await dispatchLifecycle({
      pageId: 0,
      phase: "title",
      url: "http://replaced.example/",
      title: "Edited around",
    });
    await dispatchLifecycle({
      pageId: 1,
      phase: "title",
      url: "https://other.example/",
      title: "Background",
    });
    assert.equal(input.value, "https://draft.example/keep");
    assert.ok(document.querySelector("[data-security='secure']"));
    assert.equal(document.querySelector(".tab.active .tab-title").textContent, "Edited around");
    assert.equal(document.querySelectorAll(".tab-title")[1].textContent, "Background");
    assert.ok(document.querySelector(".load-progress"));

    await act(async () => { input.blur(); });
    await dispatchLifecycle({
      pageId: 0,
      phase: "finished",
      url: "https://after-blur.example/",
      title: "",
    });
    assert.equal(input.value, "https://after-blur.example/");
    assert.equal(document.querySelector(".load-progress"), null);
    assert.equal(document.querySelectorAll(".tab-title")[1].textContent, "Background");
  });
});

async function dragTab(document, from, to) {
  const tabs = [...document.querySelectorAll(".tab")];
  const store = new Map();
  const dataTransfer = {
    effectAllowed: "move",
    dropEffect: "none",
    setData(type, value) { store.set(type, String(value)); },
    getData(type) { return store.get(type) ?? ""; },
  };
  const fire = async (element, type) => {
    const event = new window.Event(type, { bubbles: true, cancelable: true });
    Object.defineProperty(event, "dataTransfer", { value: dataTransfer });
    await act(async () => { element.dispatchEvent(event); });
  };
  await fire(tabs[from], "dragstart");
  await fire(tabs[to], "dragover");
  await fire(tabs[to], "drop");
  await fire(tabs[from], "dragend");
}

test("dragging a tab reorders the strip", async () => {
  await withApp(async ({ click, document }) => {
    await click(document.querySelector(".header > button"));
    await click(document.querySelector(".header > button"));
    assert.deepEqual([...document.querySelectorAll(".tab-title")].map((node) => node.textContent), ["Tab 1", "Tab 2", "Tab 3"]);
    await dragTab(document, 0, 2);
    assert.deepEqual([...document.querySelectorAll(".tab-title")].map((node) => node.textContent), ["Tab 2", "Tab 3", "Tab 1"]);
    await dragTab(document, 2, 0);
    assert.deepEqual([...document.querySelectorAll(".tab-title")].map((node) => node.textContent), ["Tab 1", "Tab 2", "Tab 3"]);
  });
});

test("AppKit ignores the new chrome chords and still focuses the address field", async () => {
  await withApp(async ({ calls, document, keydown }) => {
    for (const [key, options] of [
      ["r", { ctrl: true }],
      ["[", { meta: true }],
      ["]", { ctrl: true }],
      ["1", { meta: true }],
      ["9", { ctrl: true }],
      ["[", { ctrl: true, shift: true }],
      ["]", { meta: true, shift: true }],
      ["ArrowLeft", { alt: true }],
      ["ArrowRight", { alt: true }],
      ["Tab", { ctrl: true }],
      ["Tab", { ctrl: true, shift: true }],
    ]) {
      calls.length = 0;
      await keydown(key, options);
      assert.deepEqual(calls, []);
    }
    assert.equal(document.querySelectorAll(".tab").length, 1);
    await keydown("l", { meta: true });
    assert.equal(document.activeElement, document.querySelector(".url-input"));
  }, { lane: "appkit" });
});

test("Alt+Left and Alt+Right go back and forward, including from a text field", async () => {
  await withApp(async ({ calls, document, keydown }) => {
    const urlInput = document.querySelector(".url-input");
    await act(async () => { urlInput.focus(); });
    const back = await keydown("ArrowLeft", { alt: true });
    assert.equal(back.defaultPrevented, true);
    assert.deepEqual(calls, [
      ["browser", "browser_back", "session", 0],
      ["snapshot", "session", 0],
    ]);
    calls.length = 0;
    const forward = await keydown("ArrowRight", { alt: true, code: "ArrowRight" });
    assert.equal(forward.defaultPrevented, true);
    assert.deepEqual(calls, [
      ["browser", "browser_forward", "session", 0],
      ["snapshot", "session", 0],
    ]);
    calls.length = 0;
    await keydown("ArrowLeft", { alt: true, ctrl: true });
    await keydown("ArrowLeft", { alt: true, shift: true });
    await keydown("ArrowLeft", { alt: true, repeat: true });
    assert.deepEqual(calls, []);
  });
});

test("Ctrl+Tab and Ctrl+Shift+Tab select the next and previous tab", async () => {
  await withApp(async ({ calls, click, document, keydown }) => {
    await keydown("Tab", { ctrl: true });
    assert.deepEqual(calls, []);
    await openMoreTabs(click, document, 2);
    calls.length = 0;
    const next = await keydown("Tab", { ctrl: true });
    assert.equal(next.defaultPrevented, true);
    assert.deepEqual(calls, [
      ["activate", "session", 0],
      ["snapshot", "session", 0],
    ]);
    calls.length = 0;
    const previous = await keydown("Tab", { ctrl: true, shift: true });
    assert.equal(previous.defaultPrevented, true);
    assert.deepEqual(calls, [
      ["activate", "session", 2],
      ["snapshot", "session", 2],
    ]);
    const chat = document.querySelector(".chat-input");
    await act(async () => { chat.focus(); });
    calls.length = 0;
    await keydown("Tab", { ctrl: true });
    assert.deepEqual(calls, [
      ["activate", "session", 0],
      ["snapshot", "session", 0],
    ]);
    calls.length = 0;
    await keydown("Tab", { ctrl: true, meta: true });
    await keydown("Tab", { alt: true });
    assert.deepEqual(calls, []);
  });
});

test("menu commands use the same handlers as the chrome shortcuts", async () => {
  await withApp(async ({ calls, click, dispatchLifecycle, dispatchMenu, document }) => {
    await dispatchMenu("new-tab");
    assert.equal(document.querySelectorAll(".tab").length, 2);
    await click(document.querySelector(".header > button"));
    assert.equal(document.querySelectorAll(".tab").length, 3);

    calls.length = 0;
    await dispatchMenu("select-tab-2");
    assert.deepEqual(calls, [
      ["activate", "session", 1],
      ["snapshot", "session", 1],
    ]);
    assert.equal(document.querySelector(".tab.active .tab-title").textContent, "Page 1");

    calls.length = 0;
    await dispatchMenu("next-tab");
    assert.deepEqual(calls, [
      ["activate", "session", 2],
      ["snapshot", "session", 2],
    ]);
    calls.length = 0;
    await dispatchMenu("previous-tab");
    assert.deepEqual(calls, [
      ["activate", "session", 1],
      ["snapshot", "session", 1],
    ]);
    calls.length = 0;
    await dispatchMenu("select-tab-9");
    assert.deepEqual(calls, [
      ["activate", "session", 2],
      ["snapshot", "session", 2],
    ]);

    calls.length = 0;
    await dispatchMenu("back");
    assert.deepEqual(calls, [
      ["browser", "browser_back", "session", 2],
      ["snapshot", "session", 2],
    ]);
    calls.length = 0;
    await dispatchMenu("forward");
    assert.deepEqual(calls, [
      ["browser", "browser_forward", "session", 2],
      ["snapshot", "session", 2],
    ]);
    calls.length = 0;
    await dispatchMenu("reload");
    assert.deepEqual(calls, [
      ["browser", "browser_reload", "session", 2],
      ["snapshot", "session", 2],
    ]);

    await dispatchMenu("focus-address");
    assert.equal(document.activeElement, document.querySelector(".url-input"));

    calls.length = 0;
    await dispatchMenu("stop");
    assert.deepEqual(calls, []);
    await dispatchLifecycle({
      pageId: 2,
      phase: "started",
      url: "https://slow.example/",
      title: "",
    });
    calls.length = 0;
    await dispatchMenu("stop");
    assert.deepEqual(calls, [["browser", "stop_page_load", "session", 2]]);

    const tabCount = document.querySelectorAll(".tab").length;
    await dispatchMenu("close-tab");
    assert.equal(document.querySelectorAll(".tab").length, tabCount - 1);

    calls.length = 0;
    await dispatchMenu("copy");
    await dispatchMenu("paste");
    await dispatchMenu("undo");
    await dispatchMenu("reopen-closed-tab");
    assert.deepEqual(calls, []);
  });
});

test("the address bar opens URLs and searches DuckDuckGo for other text", async () => {
  await withApp(async ({ calls, document, goTo }) => {
    assert.equal(document.querySelector(".omnibox-hint"), null);

    await goTo("cats", { submit: false });
    assert.equal(document.querySelector(".omnibox-hint").textContent, "Search DuckDuckGo");
    await goTo("example.com/docs", { submit: false });
    assert.equal(document.querySelector(".omnibox-hint").textContent, "Go to");

    const navigatedTo = () => calls.filter((call) => call[0] === "navigate").map((call) => call[3]);
    await goTo("   ");
    await goTo("  example.com/docs  ");
    await goTo("foo bar.com");
    await goTo("localhost:3000/app");
    await goTo("?example.com");
    await goTo("https://already.example/path");
    assert.deepEqual(navigatedTo(), [
      "https://example.com/docs",
      "https://duckduckgo.com/?q=foo%20bar.com",
      "http://localhost:3000/app",
      "https://duckduckgo.com/?q=example.com",
      "https://already.example/path",
    ]);
    assert.match(statusText(document), /Navigating to https:\/\/already\.example\/path/);
  }, { lane: "appkit" });
});

test("a bare loopback address still reaches the host netguard check", async () => {
  const reason = "Refusing to fetch internal/loopback address (host '127.0.0.1')";
  await withApp(async ({ calls, document, goTo }) => {
    await goTo("127.0.0.1");
    assert.deepEqual(calls, [["navigate", "session", 0, "http://127.0.0.1"]]);
    assert.match(
      document.querySelector(".status-line").textContent,
      /Navigation failed: Refusing to fetch internal\/loopback address \(host '127\.0\.0\.1'\)/
    );
  }, { navigateError: reason, navigateFails: true });
});

test("a menu echo of a chrome shortcut does not run the command twice", async () => {
  await withApp(async ({ calls, dispatchLifecycle, dispatchMenu, document, keydown }) => {
    const tabsBefore = document.querySelectorAll(".tab").length;
    await keydown("t", { ctrl: true });
    await dispatchMenu("new-tab");
    assert.equal(document.querySelectorAll(".tab").length, tabsBefore + 1);

    calls.length = 0;
    await keydown("r", { meta: true });
    await dispatchMenu("reload");
    assert.deepEqual(calls, [
      ["browser", "browser_reload", "session", 1],
      ["snapshot", "session", 1],
    ]);

    const urlInput = document.querySelector(".url-input");
    await act(async () => { urlInput.focus(); });
    calls.length = 0;
    const history = await keydown("[", { ctrl: true });
    assert.equal(history.defaultPrevented, false);
    await dispatchMenu("back");
    assert.deepEqual(calls, []);

    await act(async () => { urlInput.blur(); });
    await dispatchLifecycle({
      pageId: 1,
      phase: "started",
      url: "https://slow.example/",
      title: "",
    });
    calls.length = 0;
    await keydown("Escape");
    await dispatchMenu("stop");
    assert.deepEqual(calls, [["browser", "stop_page_load", "session", 1]]);
  });
});

function sampleHistory(query) {
  const recent = [
    { url: "https://docs.example/guide", title: "Guide", visitCount: 4 },
    { url: "https://news.example/", title: "News", visitCount: 1 },
  ];
  if (!query) return recent;
  const needle = query.toLowerCase();
  return recent.filter((hit) => hit.url.includes(needle) || hit.title.toLowerCase().includes(needle));
}

async function settle(ms = 0) {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, ms));
  });
}

async function setInput(input, value) {
  const setter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, "value").set;
  await act(async () => {
    input.focus();
    const tracker = input._valueTracker;
    if (tracker) tracker.setValue("");
    setter.call(input, value);
    input.dispatchEvent(new window.Event("input", { bubbles: true }));
    input.dispatchEvent(new window.Event("keyup", { bubbles: true }));
  });
}

function submitStartSearch(document) {
  return act(async () => {
    document.querySelector(".start-search-form").dispatchEvent(
      new window.Event("submit", { bubbles: true, cancelable: true })
    );
  });
}

test("a blank tab shows a local start page and focuses the address bar", async () => {
  await withApp(async ({ calls, click, document }) => {
    await settle();
    assert.equal(document.activeElement, document.querySelector(".location .url-input"));
    assert.ok(document.querySelector(".start-page"));
    assert.equal(document.querySelectorAll(".start-site-title").length, 2);
    assert.equal(document.querySelector(".start-site-title").textContent, "Guide");
    assert.equal(document.querySelector(".start-site-host").textContent, "docs.example");
    assert.deepEqual(calls.filter((call) => call[0] === "history"), [["history", "", 8]]);

    const search = document.querySelector(".start-search");
    await setInput(search, "cats");
    assert.equal(document.querySelector(".start-search-hint").textContent, "Search DuckDuckGo");
    calls.length = 0;
    await submitStartSearch(document);
    assert.deepEqual(
      calls.filter((call) => call[0] === "navigate").map((call) => call[3]),
      ["https://duckduckgo.com/?q=cats"]
    );

    await click(document.querySelector(".header > button"));
    await settle();
    assert.ok(document.querySelector(".start-page"));
    await setInput(document.querySelector(".start-search"), "example.com/docs");
    assert.equal(document.querySelector(".start-search-hint").textContent, "Go to");
    calls.length = 0;
    await submitStartSearch(document);
    assert.deepEqual(
      calls.filter((call) => call[0] === "navigate").map((call) => call[3]),
      ["https://example.com/docs"]
    );

    await click(document.querySelector(".header > button"));
    await settle();
    calls.length = 0;
    await click(document.querySelector(".start-site"));
    assert.deepEqual(
      calls.filter((call) => call[0] === "navigate").map((call) => call[3]),
      ["https://docs.example/guide"]
    );
  }, { history: sampleHistory, keepStartFocus: true });
});

test("a new tab clears the address bar and shows the start page again", async () => {
  await withApp(async ({ dispatchLifecycle, dispatchMenu, document }) => {
    await dispatchLifecycle({
      pageId: 0,
      phase: "finished",
      url: "https://example.com/",
      title: "Example",
    });
    assert.equal(document.querySelector(".start-page"), null);
    assert.equal(document.querySelector(".url-input").value, "https://example.com/");

    await dispatchMenu("new-tab");
    assert.equal(document.querySelectorAll(".tab").length, 2);
    assert.equal(document.querySelector(".url-input").value, "");
    assert.ok(document.querySelector(".start-page"));
    assert.equal(document.activeElement, document.querySelector(".location .url-input"));
  });
});

test("address suggestions open history and Escape closes them without stopping", async () => {
  await withApp(async ({ calls, dispatchLifecycle, document, keydown }) => {
    await dispatchLifecycle({
      pageId: 0,
      phase: "started",
      url: "https://slow.example/",
      title: "",
    });
    const input = document.querySelector(".location .url-input");
    await setInput(input, "docs");
    await settle(350);
    const rows = [...document.querySelectorAll(".omnibox-suggestion")];
    assert.equal(rows[0].dataset.kind, "action");
    assert.equal(rows[0].textContent, "Search DuckDuckGo for docs");
    assert.equal(rows[1].dataset.kind, "history");
    assert.match(rows[1].textContent, /Guide/);
    assert.match(rows[1].textContent, /docs\.example/);
    assert.equal(rows[0].classList.contains("active"), true);

    calls.length = 0;
    const down = await keydown("ArrowDown", { target: input });
    assert.equal(down.defaultPrevented, true);
    assert.equal(document.querySelectorAll(".omnibox-suggestion")[1].classList.contains("active"), true);
    await act(async () => {
      document.querySelector(".url-bar").dispatchEvent(
        new window.Event("submit", { bubbles: true, cancelable: true })
      );
    });
    assert.deepEqual(
      calls.filter((call) => call[0] === "navigate").map((call) => call[3]),
      ["https://docs.example/guide"]
    );
    assert.equal(document.querySelector(".omnibox-suggestion"), null);

    await dispatchLifecycle({
      pageId: 0,
      phase: "started",
      url: "https://slow.example/again",
      title: "",
    });
    await setInput(input, "example.com/docs");
    await settle(350);
    assert.equal(document.querySelector(".omnibox-suggestion").textContent, "Go to https://example.com/docs");
    calls.length = 0;
    const closed = await keydown("Escape", { target: input });
    assert.equal(closed.defaultPrevented, true);
    assert.equal(document.querySelector(".omnibox-suggestion"), null);
    assert.equal(calls.some((call) => call[1] === "stop_page_load"), false);
    assert.equal(input.value, "example.com/docs");

    await act(async () => { input.blur(); });
    calls.length = 0;
    const stop = await keydown("Escape");
    assert.equal(stop.defaultPrevented, true);
    assert.deepEqual(calls.filter((call) => call[0] === "browser"), [
      ["browser", "stop_page_load", "session", 0],
    ]);
  }, { history: sampleHistory });
});

test("address suggestions show history and memory, and Forget still removes the page", async () => {
  const memoryHit = {
    block_id: "block-1",
    page_url: "https://notes.example/page",
    heading_path: ["Meeting notes"],
    text: "Quarterly plan for the team",
  };
  await withApp(async ({ calls, click, document, keydown }) => {
    const input = document.querySelector(".location .url-input");
    await setInput(input, "docs");
    await settle(350);

    const labels = [...document.querySelectorAll(".omnibox-suggestions-label")].map((label) => label.textContent);
    assert.deepEqual(labels, ["History", "Memory"]);
    const rows = [...document.querySelectorAll(".omnibox-suggestion")];
    assert.equal(rows[0].dataset.kind, "action");
    assert.equal(rows[0].textContent, "Search DuckDuckGo for docs");
    assert.equal(rows[1].dataset.kind, "history");
    assert.match(rows[1].textContent, /Guide/);
    assert.match(rows[1].textContent, /docs\.example/);
    assert.equal(rows[2].dataset.kind, "memory");
    assert.match(rows[2].textContent, /Meeting notes/);
    assert.match(rows[2].textContent, /https:\/\/notes\.example\/page/);
    assert.equal(rows[0].classList.contains("active"), true);

    await keydown("ArrowDown", { target: input });
    await keydown("ArrowDown", { target: input });
    const moved = [...document.querySelectorAll(".omnibox-suggestion")];
    assert.equal(moved[1].classList.contains("active"), false);
    assert.equal(moved[2].classList.contains("active"), true);

    const why = [...moved[2].querySelectorAll("button")].find((button) => button.textContent === "Why");
    await click(why);
    assert.match(document.querySelector(".omnibox-explain").textContent, /matched the heading/);
    assert.equal(document.querySelector(".omnibox-explain-score").textContent, "title 1.50");
    assert.ok(calls.some((call) => call[0] === "explain" && call[1] === "docs" && call[2] === "block-1"));

    const forget = [...document.querySelectorAll(".omnibox-suggestion-action")].find((button) => button.textContent === "Forget");
    await click(forget);
    assert.equal(document.querySelector(".omnibox-forget-prompt").textContent, "Stop capturing notes.example?");
    calls.length = 0;
    await click(document.querySelector(".omnibox-suggestion-action.danger"));
    assert.deepEqual(calls.filter((call) => call[0] === "forget"), [["forget", "https://notes.example/page"]]);
    assert.equal(document.querySelector("[data-kind='memory']"), null);
    assert.ok(document.querySelector("[data-kind='history']"));
    assert.match(
      document.querySelector(".status-bar").textContent,
      /Forgot notes\.example and blocked further capture for that site\./
    );
  }, {
    history: sampleHistory,
    memory: {
      search: [memoryHit],
      explain: {
        snippets: ["matched the heading"],
        breakdown: [{ field: "title", score: 1.5 }],
      },
    },
  });
});

test("the AppKit shell does not draw the local start page", async () => {
  await withApp(async ({ document }) => {
    assert.equal(document.querySelector(".start-page"), null);
  }, { lane: "appkit" });
});

test("a foreground popup opens a tab and closing it returns to the opener", async () => {
  await withApp(async ({ calls, click, createdPages, dispatchNewWindow, document }) => {
    assert.equal(document.querySelectorAll(".tab").length, 1);
    await dispatchNewWindow({ pageId: 0, url: "https://opened.example/a", disposition: "foreground" });
    assert.equal(document.querySelectorAll(".tab").length, 2);
    assert.equal(document.querySelector(".tab.active .tab-title").textContent, "Page 1");
    assert.deepEqual(createdPages.at(-1), null);
    assert.ok(calls.some((call) => call[0] === "navigate" && call[2] === 1 && call[3] === "https://opened.example/a"));

    await click(document.querySelector(".header > button"));
    await click(document.querySelectorAll(".tab-title")[1]);
    calls.length = 0;
    await click(document.querySelector(".tab.active .tab-close"));
    assert.equal(document.querySelector(".tab.active .tab-title").textContent, "Page 0");
    assert.deepEqual(calls[0], ["close", "session", 1]);
    assert.deepEqual(calls.find((call) => call[0] === "activate"), ["activate", "session", 0]);
  });
});

test("a background popup keeps the current tab", async () => {
  await withApp(async ({ calls, createdPages, dispatchNewWindow, document, goTo }) => {
    await goTo("https://current.example");
    calls.length = 0;
    await dispatchNewWindow({ pageId: 0, url: "https://background.example/b", disposition: "background" });
    assert.equal(document.querySelectorAll(".tab").length, 2);
    assert.equal(document.querySelector(".tab.active .tab-title").textContent, "Page 0");
    assert.equal(document.querySelector(".url-input").value, "https://page0.example");
    assert.deepEqual(createdPages.at(-1), { activate: false });
    assert.ok(calls.some((call) => call[0] === "navigate" && call[2] === 1 && call[3] === "https://background.example/b"));
    assert.equal(calls.some((call) => call[0] === "activate"), false);
  });
});

test("a blocked popup stays in the address bar until it is opened", async () => {
  await withApp(async ({ calls, click, dispatchNewWindow, document }) => {
    await dispatchNewWindow({ pageId: 0, url: "https://blocked.example/c", disposition: "blocked" });
    await dispatchNewWindow({ pageId: 0, url: "https://blocked.example/d", disposition: "blocked" });
    const notice = document.querySelector(".popup-blocked");
    assert.ok(notice);
    assert.match(notice.textContent, /2 popups blocked/);
    assert.equal(document.querySelectorAll(".tab").length, 1);

    calls.length = 0;
    await click(notice.querySelector("button"));
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 20));
    });
    assert.equal(document.querySelectorAll(".tab").length, 2);
    assert.equal(document.querySelector(".tab.active .tab-title").textContent, "Page 1");
    assert.equal(document.querySelector(".popup-blocked"), null);
    assert.ok(calls.some((call) => call[0] === "navigate" && call[3] === "https://blocked.example/c"));

    await click(document.querySelectorAll(".tab-title")[0]);
    assert.match(document.querySelector(".popup-blocked").textContent, /^Popup blocked/);
    await click(document.querySelector(".popup-blocked button[aria-label='Dismiss blocked popup']"));
    assert.equal(document.querySelector(".popup-blocked"), null);
  });
});

test("downloads show progress, done, and failed, and a finished file can be opened", async () => {
  await withApp(async ({ calls, click, dispatchDownload, document }) => {
    assert.equal(document.querySelector(".downloads"), null);
    await dispatchDownload({
      id: "dl-1",
      phase: "progress",
      url: "https://cdn.example/report.pdf",
      filename: "report.pdf",
      received: 0,
      total: null,
    });
    const row = document.querySelector(".downloads [data-phase='progress']");
    assert.equal(row.querySelector(".download-name").textContent, "report.pdf");
    assert.match(row.querySelector(".download-progress").textContent, /Downloading/);

    await dispatchDownload({
      id: "dl-1",
      phase: "progress",
      url: "https://cdn.example/report.pdf",
      filename: "report.pdf",
      received: 50,
      total: 100,
    });
    assert.match(document.querySelector(".download-progress").textContent, /50%/);

    await dispatchDownload({
      id: "dl-1",
      phase: "done",
      url: "https://cdn.example/report.pdf",
      filename: "report.pdf",
      received: 100,
      total: 100,
    });
    const done = document.querySelector(".downloads [data-phase='done']");
    assert.match(done.textContent, /Done/);
    const open = [...done.querySelectorAll("button")].find((button) => button.textContent === "Open");
    const show = [...done.querySelectorAll("button")].find((button) => button.textContent === "Show in folder");
    await click(open);
    await click(show);
    assert.deepEqual(calls.filter((call) => call[0] === "open-download" || call[0] === "show-download"), [
      ["open-download", "dl-1"],
      ["show-download", "dl-1"],
    ]);

    await dispatchDownload({
      id: "dl-2",
      phase: "failed",
      url: "http://127.0.0.1/secret.bin",
      filename: "secret.bin",
      received: 0,
      total: null,
      error: "Refusing to fetch internal/loopback address (host '127.0.0.1')",
    });
    const failed = document.querySelector(".downloads [data-phase='failed']");
    assert.match(failed.textContent, /Failed: Refusing to fetch internal\/loopback address/);
    assert.equal(failed.querySelector("button"), null);
  });
});

test("find in page shows the match count and zoom shortcuts change the page", async () => {
  const finds = [];
  const zooms = [];
  await withApp(async ({ click, dispatchLifecycle, dispatchMenu, document, keydown }) => {
    assert.equal(document.querySelector(".zoom-level").textContent, "100%");
    assert.equal(document.querySelector(".find-bar"), null);

    await keydown("f", { meta: true });
    const input = document.querySelector(".find-bar input");
    assert.ok(input);
    const setter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, "value").set;
    await act(async () => {
      setter.call(input, "cats");
      input.dispatchEvent(new window.Event("input", { bubbles: true }));
    });
    await act(async () => { await Promise.resolve(); });
    assert.equal(document.querySelector(".find-count").textContent, "2 of 4");
    assert.deepEqual(finds.at(-1).slice(2), ["cats", "start"]);

    await click(document.querySelector("[aria-label='Next match']"));
    await act(async () => { await Promise.resolve(); });
    assert.equal(document.querySelector(".find-count").textContent, "3 of 4");
    assert.equal(finds.at(-1)[3], "next");

    await dispatchMenu("find-previous");
    await act(async () => { await Promise.resolve(); });
    assert.equal(finds.at(-1)[3], "previous");

    await keydown("=", { meta: true });
    await keydown("-", { ctrl: true });
    await keydown("0", { meta: true });
    await act(async () => { await Promise.resolve(); });
    assert.deepEqual(zooms, ["in", "out", "reset"]);
    assert.equal(document.querySelector(".zoom-level").textContent, "100%");

    await dispatchLifecycle({ pageId: 0, phase: "finished", url: "https://docs.example/guide", title: "Guide" });
    await act(async () => { await Promise.resolve(); });
    assert.equal(zooms.at(-1), "get");
    assert.equal(document.querySelector(".zoom-level").textContent, "150%");

    await keydown("Escape");
    assert.equal(document.querySelector(".find-bar"), null);
    assert.equal(finds.at(-1)[2], "");
  }, {
    findInPage: async (sessionId, pageId, query, direction) => {
      finds.push([sessionId, pageId, query, direction]);
      if (!query) return { total: 0, current: 0 };
      if (direction === "next") return { total: 4, current: 3 };
      if (direction === "previous") return { total: 4, current: 2 };
      return { total: 4, current: 2 };
    },
    adjustPageZoom: async (sessionId, pageId, action) => {
      zooms.push(action);
      if (action === "get") return { zoom: 1.5, percent: 150 };
      if (action === "in") return { zoom: 1.1, percent: 110 };
      if (action === "out") return { zoom: 0.9, percent: 90 };
      return { zoom: 1, percent: 100 };
    },
  });
});

test("the browser stage stays the flexing row while find is open", async () => {
  await withApp(async ({ dispatchDownload, document, keydown }) => {
    const style = document.createElement("style");
    style.textContent = browserStyles;
    document.head.append(style);

    await keydown("f", { meta: true });
    await dispatchDownload({
      id: "dl-1",
      phase: "progress",
      url: "https://cdn.example/report.pdf",
      filename: "report.pdf",
      received: 10,
      total: 100,
    });

    const panel = document.querySelector(".browser-panel");
    const stage = document.querySelector(".browser-stage");
    const findBar = document.querySelector(".find-bar");
    const downloads = document.querySelector(".downloads");
    assert.equal(stage.parentElement, panel);
    assert.equal(findBar.parentElement, panel);
    assert.equal(downloads.parentElement, panel);
    assert.equal(document.querySelector(".downloads-slot"), null);
    assert.deepEqual([...panel.children].map((child) => child.className), [
      "find-bar",
      "downloads",
      "browser-stage",
    ]);

    const view = document.defaultView;
    const panelStyle = view.getComputedStyle(panel);
    assert.equal(panelStyle.display, "flex");
    assert.equal(panelStyle.flexDirection, "column");

    const stageStyle = view.getComputedStyle(stage);
    assert.equal(stageStyle.flexGrow, "1");
    assert.equal(stageStyle.flexBasis, "0%");
    assert.equal(stageStyle.minHeight, "0px");

    for (const child of panel.children) {
      if (child === stage) continue;
      const childStyle = view.getComputedStyle(child);
      assert.equal(childStyle.flexGrow, "0", child.className);
      assert.equal(childStyle.flexShrink, "0", child.className);
    }
  });
});


test("AppKit selecting a loading tab clears the previous tab's statistics", async () => {
  await withApp(async ({ click, dispatch, document }) => {
    await dispatch({ type: "navigation", pageId: 0, navigationId: 1 });
    await dispatch({
      type: "snapshot", pageId: 0, navigationId: 1, reason: "finish", loading: false,
      snapshot: { url: "https://first.example/", title: "First", link_count: 17, image_count: 9 },
    });
    assert.equal(document.querySelector(".stat-value").textContent, "17");
    await click(document.querySelector(".header > button"));
    await dispatch({ type: "navigation", pageId: 1, navigationId: 2 });
    await dispatch({
      type: "snapshot", pageId: 1, navigationId: 2, reason: "select", loading: true,
      snapshot: { url: "https://slow.example/", title: "Previous document", link_count: 100 },
    });
    assert.match(statusText(document), /Loading/);
    assert.deepEqual([...document.querySelectorAll(".stat-value")].map((node) => node.textContent), ["0", "0", "0", "0", "0"]);
  }, { lane: "appkit" });
});
