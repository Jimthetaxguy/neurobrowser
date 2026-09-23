import assert from "node:assert/strict";
import { test } from "node:test";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";
import { JSDOM } from "jsdom";

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
