import assert from 'node:assert/strict';
import { test } from 'node:test';
import { readFileSync } from 'node:fs';
import { JSDOM } from 'jsdom';

const source = readFileSync(new URL('./runtime.rs', import.meta.url), 'utf8');
const marker = 'const RUNTIME_INIT_SCRIPT: &str = r#"';
const start = source.indexOf(marker) + marker.length;
const script = source.slice(start, source.indexOf('"#;', start));
const runtimeId = 'page-runtime-1';
function withRuntime(html, work) {
  const dom = new JSDOM(`<!doctype html><html><body>${html}</body></html>`, {
    url: 'https://example.test/page', runScripts: 'outside-only',
  });
  try {
    dom.window.eval(script);
    work(dom.window.__NEUROBROWSER_RUNTIME__, dom.window);
  } finally { dom.window.close(); }
}
const command = (observation, target, action = 'click', text = null) => ({
  document: observation.document, target_id: target.id, action, text,
});

test('observations exclude HTML and every input value, with UTF-8 text and collection bounds', () => {
  withRuntime(`<p>😀😀😀😀😀</p><textarea>textarea-canary</textarea>
    <input name="q" value="ordinary-canary"><input type="password" value="password-canary">
    <input type="hidden" value="hidden-canary"><script>script-canary</script>
    <a href="/one">One</a><a href="/two">Two</a>
    <table><tr><td>Cell one</td><td>Cell two</td></tr><tr><td>Second row</td></tr></table>`, (runtime) => {
    const observation = runtime.observe(runtimeId, {
      max_text_bytes: 8, max_targets: 1, max_links: 1, max_tables: 1, max_rows: 1, max_cell_bytes: 4,
    });
    const serialized = JSON.stringify(observation);
    for (const secret of ['textarea-canary', 'ordinary-canary', 'password-canary', 'hidden-canary', 'script-canary'])
      assert.equal(serialized.includes(secret), false, `${secret} leaked`);
    assert.equal(observation.html, undefined);
    assert.equal(observation.forms, undefined);
    assert.ok(Buffer.byteLength(observation.text) <= 8);
    assert.equal(observation.targets.length, 1);
    assert.equal(observation.links.length, 1);
    assert.equal(observation.tables[0].rows.length, 1);
    assert.ok(observation.tables[0].rows[0].every(cell => Buffer.byteLength(cell) <= 4));
    assert.ok(observation.omissions.includes('Targets truncated'));
    assert.ok(observation.omissions.includes('Links truncated'));
    assert.equal(observation.capabilities.screenshots, false);
    assert.equal(observation.capabilities.enforcing_subresource_network, false);
  });
});

test('same-document replacement is rejected synchronously before MutationObserver callback', () => {
  withRuntime('<button id="save">Save</button>', (runtime, window) => {
    const before = runtime.observe(runtimeId);
    const oldNode = window.document.querySelector('#save');
    const nextNode = oldNode.cloneNode(true);
    oldNode.replaceWith(nextNode);
    let clicked = 0;
    nextNode.addEventListener('click', () => clicked++);
    assert.equal(runtime.dispatchTarget(command(before, before.targets[0])).state, 'not_dispatched');
    assert.equal(clicked, 0);
    const after = runtime.observe(runtimeId);
    assert.notEqual(after.targets[0].id, before.targets[0].id);
    assert.ok(after.document.revision > before.document.revision);
    assert.equal(runtime.dispatchTarget(command(after, after.targets[0])).state, 'acknowledged');
    assert.equal(clicked, 1);
    assert.equal(runtime.dispatchTarget(command(after, after.targets[0])).state, 'not_dispatched');
  });
});

test('SPA URL change invalidates reviewed state and fresh evidence names the new URL', () => {
  withRuntime('<button>Continue</button>', (runtime, window) => {
    const before = runtime.observe(runtimeId);
    window.history.pushState({}, '', '/new-view');
    assert.equal(runtime.dispatchTarget(command(before, before.targets[0])).state, 'not_dispatched');
    const after = runtime.observe(runtimeId);
    assert.equal(after.url, 'https://example.test/new-view');
    assert.ok(after.document.revision > before.document.revision);
  });
});

test('input events invalidate reviewed state even if no DOM attributes changed', () => {
  withRuntime('<input name="search" value="old">', (runtime, window) => {
    const before = runtime.observe(runtimeId);
    const input = window.document.querySelector('input');
    input.value = 'new';
    input.dispatchEvent(new window.Event('input', { bubbles: true }));
    assert.equal(runtime.dispatchTarget(command(before, before.targets[0], 'type', 'typed')).state, 'not_dispatched');
    assert.equal(input.value, 'new');
  });
});

test('property-only form value changes invalidate the target fingerprint without exposing values', () => {
  withRuntime('<form action="/send"><input name="q" value="old"><button>Send</button></form>', (runtime, window) => {
    const before = runtime.observe(runtimeId);
    const target = before.targets.find(target => target.tag === 'button');
    window.document.querySelector('input').value = 'changed-secret';
    assert.equal(runtime.dispatchTarget(command(before, target)).state, 'not_dispatched');
    assert.equal(JSON.stringify(runtime.observe(runtimeId)).includes('changed-secret'), false);
  });
});

test('target typing acknowledges dispatch and changes the real DOM value', () => {
  withRuntime('<label for="q">Search</label><input id="q">', (runtime, window) => {
    const observation = runtime.observe(runtimeId);
    const result = runtime.dispatchTarget(command(observation, observation.targets[0], 'type', 'typed value'));
    assert.equal(result.state, 'acknowledged');
    assert.equal(window.document.querySelector('input').value, 'typed value');
    assert.equal(JSON.stringify(runtime.observe(runtimeId)).includes('typed value'), false);
  });
});

test('cross-runtime, cross-document and disabled target dispatch are rejected', () => {
  withRuntime('<button disabled>Save</button>', (runtime) => {
    const observation = runtime.observe(runtimeId);
    const validCommand = command(observation, observation.targets[0]);
    assert.equal(runtime.dispatchTarget(validCommand).state, 'not_dispatched');
    assert.equal(runtime.dispatchTarget({ ...validCommand, document: { ...validCommand.document, runtime_id: 'page-runtime-2' } }).state, 'not_dispatched');
    assert.equal(runtime.dispatchTarget({ ...validCommand, document: { ...validCommand.document, document_id: 'other-document' } }).state, 'not_dispatched');
  });
});

test('an exception after possible dispatch reports uncertainty and invalidates replay', () => {
  withRuntime('<button>Save</button>', (runtime, window) => {
    const observation = runtime.observe(runtimeId);
    let effects = 0;
    window.document.querySelector('button').click = () => { effects++; throw new Error('ack lost'); };
    const intent = command(observation, observation.targets[0]);
    assert.equal(runtime.dispatchTarget(intent).state, 'unknown');
    assert.equal(effects, 1);
    assert.equal(runtime.dispatchTarget(intent).state, 'not_dispatched');
    assert.equal(effects, 1);
  });
});

test('caller limits cannot expand collection ceilings and zero limits remain valid', () => {
  withRuntime(Array.from({ length: 100 }, (_, i) => `<button>Button ${i}</button>`).join(''), (runtime) => {
    assert.equal(runtime.observe(runtimeId, { max_targets: 9999 }).targets.length, 80);
    const empty = runtime.observe(runtimeId, { max_text_bytes: 0, max_targets: 0, max_links: 0, max_tables: 0 });
    assert.equal(empty.text, '');
    assert.equal(empty.targets.length, 0);
    assert.ok(empty.omissions.includes('Targets truncated'));
  });
});

test('installed runtime methods and global reference resist direct reassignment', () => {
  withRuntime('<button>Save</button>', (runtime, window) => {
    assert.equal(Object.isFrozen(runtime), true);
    const descriptor = Object.getOwnPropertyDescriptor(window, '__NEUROBROWSER_RUNTIME__');
    assert.equal(descriptor.writable, false);
    assert.equal(descriptor.configurable, false);
    window.eval('window.__NEUROBROWSER_RUNTIME__ = { observe() { return {}; } };');
    window.eval('window.__NEUROBROWSER_RUNTIME__.dispatchTarget = () => ({ state: "acknowledged" });');
    assert.equal(window.__NEUROBROWSER_RUNTIME__, runtime);
    assert.equal(runtime.dispatchTarget({}).state, 'not_dispatched');
  });
});

test('oversized forms cannot dispatch with only a partial reviewed value fingerprint', () => {
  withRuntime(`<form>${Array.from({ length: 501 }, (_, i) => `<input name="field${i}">`).join('')}<button>Send</button></form>`, (runtime) => {
    const observation = runtime.observe(runtimeId);
    const form = observation.targets.find(target => target.tag === 'form');
    const result = runtime.dispatchTarget(command(observation, form, 'submit'));
    assert.equal(result.state, 'not_dispatched');
    assert.match(result.message, /freshness validation limit/);
  });
});

test('re-observation advances revision before refreshing property-only changed fingerprints', () => {
  withRuntime('<form action="/send"><input name="q" value="old"><button>Send</button></form>', (runtime, window) => {
    const reviewed = runtime.observe(runtimeId);
    const target = reviewed.targets.find(target => target.tag === 'button');
    window.document.querySelector('input').value = 'changed-private-value';
    const current = runtime.observe(runtimeId);
    assert.ok(current.document.revision > reviewed.document.revision);
    assert.equal(runtime.dispatchTarget(command(reviewed, target)).state, 'not_dispatched');
    assert.equal(JSON.stringify(current).includes('changed-private-value'), false);
  });
});

test('narrow observations preserve freshness comparisons for previously observed targets', () => {
  withRuntime('<form action="/send"><input name="q" value="old"><button>Send</button></form>', (runtime, window) => {
    const reviewed = runtime.observe(runtimeId);
    const target = reviewed.targets.find(target => target.tag === 'button');
    runtime.observe(runtimeId, { max_targets: 0 });
    window.document.querySelector('input').value = 'changed';
    const current = runtime.observe(runtimeId);
    assert.ok(current.document.revision > reviewed.document.revision);
    assert.equal(runtime.dispatchTarget(command(reviewed, target)).state, 'not_dispatched');
  });
});

test('oversized authority destinations are omitted instead of truncated to a different URL', () => {
  withRuntime(`<a href="/${'x'.repeat(5000)}">Long destination</a><button>Save</button>`, (runtime) => {
    const observation = runtime.observe(runtimeId);
    assert.equal(observation.targets.some(target => target.tag === 'a'), false);
    assert.ok(observation.omissions.includes('Oversized destination targets excluded'));
  });
});

test('table header rows do not consume the bounded data row allowance', () => {
  withRuntime('<table><thead><tr><th>Name</th></tr></thead><tbody><tr><td>First</td></tr><tr><td>Second</td></tr></tbody></table>', (runtime) => {
    const observation = runtime.observe(runtimeId, { max_rows: 1 });
    assert.equal(observation.tables[0].headers[0], 'Name');
    assert.deepEqual(Array.from(observation.tables[0].rows[0]), ['First']);
    assert.equal(observation.tables[0].rows.length, 1);
  });
});

test('document IDs remain random on HTTP contexts without crypto.randomUUID', () => {
  const ids = [];
  for (let i = 0; i < 2; i++) {
    const dom = new JSDOM('<!doctype html><html><body>HTTP evidence</body></html>', {
      url: 'http://example.test/page', runScripts: 'outside-only',
    });
    try {
      Object.defineProperty(dom.window.crypto, 'randomUUID', { value: undefined });
      dom.window.eval(script);
      const observation = dom.window.__NEUROBROWSER_RUNTIME__.observe(runtimeId);
      assert.match(observation.document.document_id, /^[a-f0-9]{32}$/);
      ids.push(observation.document.document_id);
    } finally { dom.window.close(); }
  }
  assert.notEqual(ids[0], ids[1]);
});

test('submission uses the exact reviewed submitter including its overridden destination', () => {
  withRuntime('<form action="/default"><button id="send" formaction="/reviewed">Send</button></form>', (runtime, window) => {
    // jsdom does not implement the formAction reflection property; expose
    // that DOM property in this focused test. Real WebKit covers reflection.
    const button = window.document.querySelector('#send');
    Object.defineProperty(button, 'formAction', { get: () => 'https://example.test/reviewed' });
    const observation = runtime.observe(runtimeId);
    const submitter = observation.targets.find(target => target.tag === 'button');
    assert.equal(submitter.destination, 'https://example.test/reviewed');
    let submittedWith;
    const form = window.document.querySelector('form');
    form.requestSubmit = (button) => { submittedWith = button; };
    assert.equal(runtime.dispatchTarget(command(observation, submitter, 'submit')).state, 'acknowledged');
    assert.equal(submittedWith, window.document.querySelector('#send'));
  });
});

test('submit rejects arbitrary descendants whose click destination differs from form action', () => {
  withRuntime('<form action="/send"><a href="/elsewhere">Link</a><input name="q"><button type="button">Other</button></form>', (runtime) => {
    const observation = runtime.observe(runtimeId);
    for (const target of observation.targets.filter(target => target.tag !== 'form')) {
      assert.equal(runtime.dispatchTarget(command(observation, target, 'submit')).state, 'not_dispatched');
    }
  });
});

test('overridden submitter destination is rejected when requestSubmit is unavailable', () => {
  withRuntime('<form action="/default"><button formaction="/reviewed">Send</button></form>', (runtime, window) => {
    const button = window.document.querySelector('button');
    Object.defineProperty(button, 'formAction', { get: () => 'https://example.test/reviewed' });
    const observation = runtime.observe(runtimeId);
    const submitter = observation.targets.find(target => target.tag === 'button');
    const form = window.document.querySelector('form');
    form.requestSubmit = undefined;
    let submitted = 0;
    form.submit = () => { submitted++; };
    const result = runtime.dispatchTarget(command(observation, submitter, 'submit'));
    assert.equal(result.state, 'not_dispatched');
    assert.match(result.message, /overridden destination/);
    assert.equal(submitted, 0);
  });
});
