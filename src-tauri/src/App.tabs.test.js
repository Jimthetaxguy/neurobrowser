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
  const loadGenerations = [];
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
    navigate: async (session, page, url, loadGeneration) => {
      calls.push(["navigate", session, page, url]);
      if (Number.isInteger(loadGeneration)) loadGenerations.push(loadGeneration);
      if (navigateFails) throw new Error("Host refused navigation");
    },
    browserAction: async (command, session, page, loadGeneration) => {
      calls.push(["browser", command, session, page]);
      if (Number.isInteger(loadGeneration)) loadGenerations.push(loadGeneration);
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
    await check({ calls, click, dispatch, document, goTo, keydown, loadGenerations, toolbar });
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

test("AppKit navigation keeps an in-flight status until a snapshot or rejection for that load", async () => {
  await withApp(async ({ calls, dispatch, document, goTo, loadGenerations, toolbar }) => {
    await goTo("https://next.example");
    assert.equal(calls.some((call) => call[0] === "snapshot"), false);
    let status = document.querySelector(".status-bar").textContent;
    assert.match(status, /Navigating to https:\/\/next\.example/);
    assert.doesNotMatch(status, /Loaded:/);
    assert.equal(document.querySelector(".url-input").value, "https://next.example");
    const firstGeneration = loadGenerations.at(-1);

    await dispatch({
      type: "status",
      message: "Only http and https URLs can be opened",
      loadGeneration: firstGeneration,
    });
    await dispatch({
      type: "snapshot",
      pageId: 0,
      loadGeneration: firstGeneration,
      snapshot: { url: "https://old.example", title: "Old title" },
    });
    status = document.querySelector(".status-bar").textContent;
    assert.match(status, /Only http and https URLs can be opened/);
    assert.doesNotMatch(status, /Loaded:/);
    assert.equal(document.querySelector(".url-input").value, "https://next.example");

    await goTo("https://newer.example");
    const newerGeneration = loadGenerations.at(-1);
    assert.notEqual(newerGeneration, firstGeneration);
    await dispatch({
      type: "snapshot",
      pageId: 0,
      loadGeneration: firstGeneration,
      snapshot: { url: "https://old.example", title: "Old title" },
    });
    status = document.querySelector(".status-bar").textContent;
    assert.match(status, /Navigating to https:\/\/newer\.example/);
    assert.doesNotMatch(status, /Loaded: Old title/);
    assert.equal(document.querySelector(".url-input").value, "https://newer.example");

    await dispatch({
      type: "snapshot",
      pageId: 0,
      loadGeneration: newerGeneration,
      snapshot: { url: "https://newer.example/", title: "Newer" },
    });
    status = document.querySelector(".status-bar").textContent;
    assert.match(status, /Loaded: Newer/);
    assert.equal(document.querySelector(".url-input").value, "https://newer.example/");

    const beforeBack = calls.length;
    await toolbar("Back");
    assert.equal(calls.slice(beforeBack).some((call) => call[0] === "snapshot"), false);
    status = document.querySelector(".status-bar").textContent;
    assert.match(status, /Going back…/);
    assert.doesNotMatch(status, /Loaded:/);
    await dispatch({
      type: "snapshot",
      pageId: 0,
      loadGeneration: loadGenerations.at(-1) - 1,
      snapshot: { url: "https://old.example", title: "Old title" },
    });
    status = document.querySelector(".status-bar").textContent;
    assert.match(status, /Going back…/);
    assert.equal(document.querySelector(".url-input").value, "https://newer.example/");
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
