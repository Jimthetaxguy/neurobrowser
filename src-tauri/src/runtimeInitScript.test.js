import assert from "node:assert/strict";
import { test } from "node:test";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import path from "node:path";
import { JSDOM } from "jsdom";

const require = createRequire(import.meta.url);
const { implForWrapper } = require("jsdom/lib/generated/idl/utils.js");
const { implementation: EventTargetImpl } = require("jsdom/lib/jsdom/living/events/EventTarget-impl.js");

// jsdom's dispatchEvent forces isTrusted false, and the getter is not configurable.
// Mark the impl and restore the flag inside dispatch, before listeners run.
const trustedEventImpls = new WeakSet();
const originalDispatchEvent = EventTargetImpl.prototype.dispatchEvent;
EventTargetImpl.prototype.dispatchEvent = function dispatchEvent(eventImpl) {
  if (!trustedEventImpls.has(eventImpl)) {
    return originalDispatchEvent.call(this, eventImpl);
  }
  const dispatch = this._dispatch;
  this._dispatch = function (impl, ...rest) {
    impl.isTrusted = true;
    return dispatch.call(this, impl, ...rest);
  };
  try {
    return originalDispatchEvent.call(this, eventImpl);
  } finally {
    this._dispatch = dispatch;
  }
};

// RUNTIME_INIT_SCRIPT is a JS string embedded in a Rust raw string literal
// (it runs inside a Tauri webview, not under Node), so there is nothing to
// `import` here. Instead we pull the literal script text out of runtime.rs
// and execute it against a real DOM (jsdom), so this test exercises exactly
// what ships rather than a hand-copied stand-in that can drift from it.
const here = path.dirname(fileURLToPath(import.meta.url));
const runtimeRsPath = path.join(here, "runtime.rs");
const runtimeRsSource = readFileSync(runtimeRsPath, "utf8");

function extractRuntimeInitScript(source) {
  const marker = 'const RUNTIME_INIT_SCRIPT: &str = r#"';
  const start = source.indexOf(marker);
  assert.notEqual(start, -1, "could not find RUNTIME_INIT_SCRIPT in runtime.rs");
  const bodyStart = start + marker.length;
  const end = source.indexOf('"#;', bodyStart);
  assert.notEqual(end, -1, "could not find end of the RUNTIME_INIT_SCRIPT raw string");
  return source.slice(bodyStart, end);
}

const runtimeInitScript = extractRuntimeInitScript(runtimeRsSource);

/** Runs RUNTIME_INIT_SCRIPT against a fresh jsdom document and returns runtime.snapshot(). */
function snapshotOf(bodyHtml) {
  const dom = new JSDOM(`<!doctype html><html><body>${bodyHtml}</body></html>`, {
    url: "https://example.test/page",
    runScripts: "outside-only",
  });
  try {
    // No window.__TAURI_INTERNALS__ in jsdom: `invoke` resolves to undefined,
    // which is fine because snapshot() never calls dispatch()/invoke().
    dom.window.eval(runtimeInitScript);
    const runtime = dom.window.__NEUROBROWSER_RUNTIME__;
    assert.ok(runtime, "runtime init script did not install window.__NEUROBROWSER_RUNTIME__");
    return runtime.snapshot();
  } finally {
    dom.window.close();
  }
}

const FORM_WITH_SECRETS = `
  <form>
    <input type="password" name="pw" value="secret">
    <input type="hidden" name="csrf" value="tok">
    <input type="text" name="username" value="alice">
  </form>
`;

test("snapshot html omits password and hidden input values but keeps normal ones", () => {
  const snapshot = snapshotOf(FORM_WITH_SECRETS);

  assert.equal(typeof snapshot.html, "string", "expected a non-null html snapshot");
  assert.equal(snapshot.html.includes("secret"), false, "password value leaked into snapshot html");
  assert.equal(snapshot.html.includes("tok"), false, "hidden input value leaked into snapshot html");
  assert.ok(snapshot.html.includes('name="username"'), "unrelated username input was dropped");
  assert.ok(snapshot.html.includes('value="alice"'), "normal text input value was redacted");
});

test("snapshot html keeps the password/hidden inputs themselves, only strips value", () => {
  const snapshot = snapshotOf(FORM_WITH_SECRETS);

  assert.ok(snapshot.html.includes('name="pw"'), "password input element was dropped, not just its value");
  assert.ok(snapshot.html.includes('name="csrf"'), "hidden input element was dropped, not just its value");
});

test("snapshot html has no value attribute at all on password/hidden inputs", () => {
  const dom = new JSDOM(`<!doctype html><html><body>${FORM_WITH_SECRETS}</body></html>`);
  const { document } = dom.window;
  const snapshot = snapshotOf(FORM_WITH_SECRETS);

  const rendered = new JSDOM(snapshot.html).window.document;
  assert.equal(rendered.querySelector('input[name="pw"]').hasAttribute("value"), false);
  assert.equal(rendered.querySelector('input[name="csrf"]').hasAttribute("value"), false);
  assert.equal(rendered.querySelector('input[name="username"]').getAttribute("value"), "alice");
  // Sanity check against the source fixture so this test would fail loudly
  // if the fixture itself stopped declaring a value attribute.
  assert.equal(document.querySelector('input[name="pw"]').getAttribute("value"), "secret");
});

test("snapshot forms structured view also redacts password and hidden values", () => {
  const snapshot = snapshotOf(FORM_WITH_SECRETS);

  const [form] = snapshot.forms;
  const byName = Object.fromEntries(form.inputs.map((input) => [input.name, input.value]));
  assert.equal(byName.pw, null);
  assert.equal(byName.csrf, null);
  assert.equal(byName.username, "alice");
});

test("querySelector attributes view also redacts password and hidden values", () => {
  const dom = new JSDOM(`<!doctype html><html><body>${FORM_WITH_SECRETS}</body></html>`, {
    url: "https://example.test/page",
    runScripts: "outside-only",
  });
  try {
    dom.window.eval(runtimeInitScript);
    const runtime = dom.window.__NEUROBROWSER_RUNTIME__;
    const [pw, csrf, username] = runtime.querySelector("input");

    assert.equal(pw.attributes.value, undefined);
    assert.equal(csrf.attributes.value, undefined);
    assert.equal(username.attributes.value, "alice");
  } finally {
    dom.window.close();
  }
});

test("a page without a documentElement yields a null html snapshot", () => {
  // Mirrors the pre-existing `root ? ... : null` null-root handling.
  const dom = new JSDOM("<!doctype html><html><body></body></html>", {
    url: "https://example.test/page",
    runScripts: "outside-only",
  });
  try {
    dom.window.eval(runtimeInitScript);
    dom.window.document.documentElement.remove();
    const runtime = dom.window.__NEUROBROWSER_RUNTIME__;
    assert.equal(runtime.snapshot().html, null);
  } finally {
    dom.window.close();
  }
});

const FORM_WITH_TEMPLATE_SECRETS = `
  <template id="outer-tpl">
    <input type="password" name="tpl-pw" value="tpl-secret">
    <input type="hidden" name="tpl-csrf" value="tpl-tok">
    <template id="inner-tpl">
      <input type="password" name="nested-pw" value="nested-secret">
      <input type="hidden" name="nested-csrf" value="nested-tok">
    </template>
  </template>
`;

test("snapshot html redacts password/hidden values inside <template>, including nested templates", () => {
  // <template> children live in a separate, inert `content` DocumentFragment
  // that ordinary querySelectorAll never descends into, so this exercises a
  // path distinct from the top-level-form test above.
  const snapshot = snapshotOf(FORM_WITH_TEMPLATE_SECRETS);

  assert.equal(snapshot.html.includes("tpl-secret"), false, "template password value leaked into snapshot html");
  assert.equal(snapshot.html.includes("tpl-tok"), false, "template hidden value leaked into snapshot html");
  assert.equal(snapshot.html.includes("nested-secret"), false, "nested-template password value leaked into snapshot html");
  assert.equal(snapshot.html.includes("nested-tok"), false, "nested-template hidden value leaked into snapshot html");
  // The inputs themselves (just not their secret values) must survive.
  assert.ok(snapshot.html.includes('name="tpl-pw"'), "templated password input element was dropped, not just its value");
  assert.ok(snapshot.html.includes('name="tpl-csrf"'), "templated hidden input element was dropped, not just its value");
  assert.ok(snapshot.html.includes('name="nested-pw"'), "nested-templated password input element was dropped, not just its value");
  assert.ok(snapshot.html.includes('name="nested-csrf"'), "nested-templated hidden input element was dropped, not just its value");
});

test("snapshot never reconstructs custom elements (no clone-triggered constructor side effects)", () => {
  const dom = new JSDOM(
    `<!doctype html><html><body><counting-element id="ce"></counting-element></body></html>`,
    { url: "https://example.test/page", runScripts: "outside-only" },
  );
  try {
    // A custom element that counts constructions and throws on the second
    // one, so any code path that re-constructs the page's custom elements
    // (e.g. cloning the live tree) is caught red-handed rather than merely
    // suspected.
    dom.window.eval(`
      window.__constructCount = 0;
      class CountingElement extends HTMLElement {
        constructor() {
          super();
          window.__constructCount += 1;
          if (window.__constructCount > 1) {
            throw new Error('CountingElement constructed a second time');
          }
        }
      }
      customElements.define('counting-element', CountingElement);
    `);
    // customElements.define() upgrades the pre-existing <counting-element>
    // synchronously: that is construction #1, expected before snapshot() is
    // ever called.
    assert.equal(dom.window.__constructCount, 1, "test setup: initial upgrade did not run as expected");

    dom.window.eval(runtimeInitScript);
    const runtime = dom.window.__NEUROBROWSER_RUNTIME__;

    let snapshot;
    assert.doesNotThrow(() => {
      snapshot = runtime.snapshot();
    }, "snapshot() must not trigger the page's custom element constructor again");

    assert.equal(
      dom.window.__constructCount,
      1,
      "snapshot() constructed the custom element again; it must only read the existing tree",
    );
    assert.ok(
      snapshot.html.includes("counting-element"),
      "custom element markup was dropped from the snapshot instead of just left un-upgraded",
    );
  } finally {
    dom.window.close();
  }
});

test("snapshot html omits iframe[srcdoc] entirely, including any secret inside it", () => {
  // srcdoc is an opaque attribute string, not a descendant tree: a
  // password input declared inside it is invisible to querySelectorAll on
  // the outer document, so it can only be handled by dropping the
  // attribute outright (sanitizing it would mean parsing it as HTML).
  const snapshot = snapshotOf(
    `<iframe srcdoc="<input type='password' value='secret'>"></iframe>`,
  );

  assert.equal(snapshot.html.includes("srcdoc"), false, "srcdoc attribute leaked into snapshot html");
  assert.equal(snapshot.html.includes("secret"), false, "secret embedded in iframe srcdoc leaked into snapshot html");
  assert.ok(snapshot.html.includes("<iframe"), "iframe element itself was dropped, not just its srcdoc");
});

test("snapshot succeeds and matches its normal output even with DOMParser/innerHTML/createContextualFragment blocked (Trusted Types simulation)", () => {
  // Simulates a page whose CSP sets `require-trusted-types-for 'script'`
  // with no default policy: every one of these three sinks throws a
  // TypeError on any plain-string input, the same way a real Trusted
  // Types violation does. The serializer must not use any of them.
  const dom = new JSDOM(
    `<!doctype html><html><body>` +
      `<input type="password" value="secret">` +
      `<input type="text" name="username" value="alice">` +
      `</body></html>`,
    { url: "https://example.test/page", runScripts: "outside-only" },
  );
  try {
    dom.window.eval(runtimeInitScript);
    const runtime = dom.window.__NEUROBROWSER_RUNTIME__;

    // The expected output is this same (already-verified-safe) call,
    // taken before any sink is blocked, so this test asserts "still
    // works, identical result" rather than duplicating a hand-written
    // expected string that could drift from the real serializer.
    const expected = runtime.snapshot().html;
    assert.ok(expected.includes('name="username"'));
    assert.equal(expected.includes("secret"), false);

    const { window } = dom;
    const blocked = () => {
      throw new TypeError("This document requires 'TrustedHTML' assignment.");
    };
    window.DOMParser = function BlockedDOMParser() {
      blocked();
    };
    const innerHTMLDescriptor = Object.getOwnPropertyDescriptor(window.Element.prototype, "innerHTML");
    Object.defineProperty(window.Element.prototype, "innerHTML", {
      configurable: true,
      get: innerHTMLDescriptor.get,
      set: blocked,
    });
    window.Range.prototype.createContextualFragment = blocked;

    let snapshot;
    assert.doesNotThrow(() => {
      snapshot = runtime.snapshot();
    }, "snapshot() used a DOMParser/innerHTML-setter/createContextualFragment sink");

    assert.equal(snapshot.html, expected, "blocking the sinks changed snapshot() output");
  } finally {
    dom.window.close();
  }
});

test("serializer output matches root.outerHTML exactly for a secret-free page (parity)", () => {
  const dom = new JSDOM(`<!doctype html><html><body></body></html>`, {
    url: "https://example.test/page",
    runScripts: "outside-only",
  });
  try {
    dom.window.eval(runtimeInitScript);
    // Built with real DOM calls (not an HTML string) so every character
    // below is exactly the character under test, with no entity-decoding
    // round trip through an HTML parser to reason about.
    dom.window.eval(`
      var NBSP = String.fromCharCode(160);
      var wrap = document.createElement('div');
      wrap.id = 'wrap';
      wrap.setAttribute('data-note', 'He said "hi" & left <now>');
      wrap.setAttribute('title', 'a<b>c');
      wrap.appendChild(document.createTextNode('Text with & entity, <tag-looking> chars, and a' + NBSP + 'nbsp.'));

      wrap.appendChild(document.createElement('br'));

      var img = document.createElement('img');
      img.setAttribute('src', 'x.png');
      img.setAttribute('alt', 'a "pic" & <thing>');
      wrap.appendChild(img);

      var scriptEl = document.createElement('script');
      scriptEl.textContent = 'if (a < b && c > "d") { /* raw & unescaped */ }';
      wrap.appendChild(scriptEl);

      var styleEl = document.createElement('style');
      styleEl.textContent = 'a[data-x="y"] { content: "<tag>&amp;"; }';
      wrap.appendChild(styleEl);

      wrap.appendChild(document.createComment(' a comment with -- dashes & <tags> '));

      var tpl = document.createElement('template');
      tpl.innerHTML = '<span>templated <b>content</b> &amp; more</span>';
      wrap.appendChild(tpl);

      var svgHost = document.createElement('div');
      svgHost.innerHTML = '<svg viewBox="0 0 10 10"><linearGradient id="g"><stop offset="0"></stop></linearGradient></svg>';
      wrap.appendChild(svgHost.firstElementChild);

      class ParityCountingElement extends HTMLElement {}
      customElements.define('parity-counting-element', ParityCountingElement);
      var custom = document.createElement('parity-counting-element');
      custom.id = 'ce';
      wrap.appendChild(custom);

      document.body.appendChild(wrap);
    `);

    const expectedHtml = dom.window.document.documentElement.outerHTML;
    const runtime = dom.window.__NEUROBROWSER_RUNTIME__;
    const snapshot = runtime.snapshot();

    assert.equal(snapshot.html, expectedHtml);
  } finally {
    dom.window.close();
  }
});

test("snapshot html drops noscript content entirely when it arrives as one raw text node (real-engine parse shape)", () => {
  // Drop noscript children because escaping does not redact them.
  const dom = new JSDOM(`<!doctype html><html><body></body></html>`, {
    url: "https://example.test/page",
    runScripts: "outside-only",
  });
  try {
    dom.window.eval(runtimeInitScript);
    dom.window.eval(`
      var ns = document.createElement('noscript');
      ns.setAttribute('data-x', 'y');
      ns.appendChild(document.createTextNode('<input type="password" value="secret">'));
      document.body.appendChild(ns);
    `);

    const runtime = dom.window.__NEUROBROWSER_RUNTIME__;
    const snapshot = runtime.snapshot();

    assert.equal(
      snapshot.html.includes("secret"),
      false,
      "credential text inside noscript's raw text node leaked into snapshot html",
    );
    assert.ok(
      snapshot.html.includes('<noscript data-x="y"></noscript>'),
      "noscript should keep its own attributes but serialize with no content",
    );
  } finally {
    dom.window.close();
  }
});

function installPopupPage(html, { gesture = true } = {}) {
  const dom = new JSDOM(`<!doctype html><html><body>${html}</body></html>`, {
    url: "https://example.test/page",
    runScripts: "outside-only",
  });
  const opened = [];
  const reports = [];
  dom.window.open = (...args) => {
    opened.push(args);
    return { closed: false };
  };
  dom.window.__TAURI_INTERNALS__ = {
    invoke: (command, args) => {
      reports.push({ command, args });
      return Promise.resolve();
    },
  };
  Object.defineProperty(dom.window.navigator, "userActivation", {
    configurable: true,
    value: { isActive: gesture },
  });
  dom.window.eval(runtimeInitScript);
  return { dom, opened, reports };
}

function trustedMouse(dom, type, init) {
  const event = new dom.window.MouseEvent(type, { bubbles: true, cancelable: true, ...init });
  trustedEventImpls.add(implForWrapper(event));
  return event;
}

test("target=_blank and modified clicks ask the shell for a tab", () => {
  const { dom, opened } = installPopupPage(
    '<a id="blank" href="https://example.com/next" target="_blank">Next</a><a id="same" href="https://example.com/same">Same</a>',
  );
  const blank = dom.window.document.getElementById("blank");
  const same = dom.window.document.getElementById("same");
  blank.dispatchEvent(trustedMouse(dom, "pointerdown", { button: 0 }));
  blank.dispatchEvent(trustedMouse(dom, "click", { button: 0 }));
  same.dispatchEvent(trustedMouse(dom, "pointerdown", { button: 0 }));
  assert.equal(opened.length, 1);
  assert.match(opened[0][0], /disposition=foreground/);
  assert.match(opened[0][0], /url=https%3A%2F%2Fexample\.com%2Fnext/);

  const modified = trustedMouse(dom, "pointerdown", { button: 0, ctrlKey: true });
  same.dispatchEvent(modified);
  assert.equal(modified.defaultPrevented, true);
  assert.match(opened[1][0], /disposition=background/);

  const middle = trustedMouse(dom, "pointerdown", { button: 1 });
  same.dispatchEvent(middle);
  assert.match(opened[2][0], /disposition=background/);
  dom.window.close();
});

test("a script-dispatched click on a target=_blank link opens no tab", () => {
  const { dom, opened } = installPopupPage(
    '<a id="blank" href="https://example.com/next" target="_blank">Next</a>',
  );
  const blank = dom.window.document.getElementById("blank");
  const event = new dom.window.MouseEvent("click", { bubbles: true, cancelable: true, button: 0 });
  assert.equal(event.isTrusted, false);
  blank.dispatchEvent(event);
  assert.equal(event.defaultPrevented, true);
  assert.equal(opened.length, 0);
  dom.window.close();
});

test("window.open without a user gesture is reported and does not open", () => {
  const { dom, opened, reports } = installPopupPage("<p>Hi</p>", { gesture: false });
  const result = dom.window.open("https://example.com/popup");
  assert.equal(result, null);
  assert.equal(opened.length, 0);
  assert.equal(reports.length, 1);
  assert.equal(reports[0].command, "browser_runtime_report");
  assert.equal(reports[0].args.payload.request_id, "popup-blocked");
  assert.equal(reports[0].args.payload.payload.url, "https://example.com/popup");
  dom.window.close();
});

test("window.open from a user gesture asks for a foreground tab", () => {
  const { dom, opened, reports } = installPopupPage("<p>Hi</p>", { gesture: true });
  dom.window.open("https://example.com/popup");
  assert.equal(reports.length, 0);
  assert.equal(opened.length, 1);
  assert.match(opened[0][0], /disposition=foreground/);
  assert.match(opened[0][0], /url=https%3A%2F%2Fexample\.com%2Fpopup/);
  dom.window.close();
});

test("snapshot html drops noscript content entirely when it parses into real child elements (jsdom's parse shape)", () => {
  // Drop noscript children because escaping does not redact them.
  const snapshot = snapshotOf(`<noscript><input type="password" value="secret"></noscript>`);

  assert.equal(
    snapshot.html.includes("secret"),
    false,
    "credential inside noscript's child elements leaked into snapshot html",
  );
  assert.ok(
    snapshot.html.includes("<noscript></noscript>"),
    "noscript should serialize with no content at all",
  );
});
