use async_trait::async_trait;
use neurobrowser::capability::{
    DispatchState, ObservationLimits, PageObservation, RuntimeCapabilities, TargetCommand,
    TargetDispatchError,
};
use neurobrowser::{browser::enrich_snapshot, BrowserInterface, ElementInfo, PageSnapshot};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::webview::{PageLoadEvent, WebviewBuilder};
use tauri::{AppHandle, LogicalPosition, LogicalSize, Manager, Webview, WebviewUrl, Window};
use tokio::sync::oneshot;
use tokio::time::sleep;

const ABOUT_BLANK_URL: &str = "about:blank";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
// Page webviews load arbitrary sites. This script must stay so host-driven
// eval can post snapshot/action results through `browser_runtime_report`.
// ACL is report-only on `page-runtime-*` (see capabilities/page-runtime.json);
// page JS must not inherit the control-surface host command set.
const RUNTIME_INIT_SCRIPT: &str = r#"
(() => {
  if (window.__NEUROBROWSER_RUNTIME__) {
    return;
  }

  const invoke = window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke;

  const limitText = (value, max = 1000) => {
    if (typeof value !== 'string') {
      return '';
    }
    return value.trim().slice(0, max);
  };

  // Shared definition of "this input's value is a secret" so the structured
  // (attrsToObject/collectForms) and raw-HTML (serializeSanitizedHtml)
  // redaction paths cannot drift apart.
  const isSecretInputType = (type) => type === 'password' || type === 'hidden';

  const attrsToObject = (element) => {
    const attrs = {};
    if (!element || !element.attributes) {
      return attrs;
    }
    const type = ((element.getAttribute && element.getAttribute('type')) || '').toLowerCase();
    for (const attr of Array.from(element.attributes)) {
      if (isSecretInputType(type) && attr.name.toLowerCase() === 'value') {
        continue;
      }
      attrs[attr.name] = attr.value;
    }
    return attrs;
  };

  const serializeElement = (element, selector) => ({
    tag: element.tagName ? element.tagName.toLowerCase() : '',
    id: element.id || null,
    classes: Array.from(element.classList || []),
    text: limitText(element.innerText || element.textContent || '', 220),
    attributes: attrsToObject(element),
    selector
  });

  const collectForms = () =>
    Array.from(document.querySelectorAll('form')).slice(0, 40).map((form) => ({
      action: form.getAttribute('action') || '',
      method: form.getAttribute('method') || 'get',
      inputs: Array.from(form.querySelectorAll('input, textarea, select, button'))
        .slice(0, 80)
        .map((input) => {
          const inputType = (input.getAttribute('type') || input.tagName.toLowerCase()).toLowerCase();
          return {
            name: input.getAttribute('name') || '',
            input_type: input.getAttribute('type') || input.tagName.toLowerCase(),
            value: isSecretInputType(inputType)
              ? null
              : (typeof input.value === 'string' ? limitText(input.value, 200) : null)
          };
        })
    }));

  const collectTables = () =>
    Array.from(document.querySelectorAll('table')).slice(0, 20).map((table) => ({
      headers: Array.from(table.querySelectorAll('th')).slice(0, 32).map((cell) => limitText(cell.innerText || cell.textContent || '', 120)),
      rows: Array.from(table.querySelectorAll('tr')).slice(0, 64).map((row) =>
        Array.from(row.querySelectorAll('td')).slice(0, 24).map((cell) => limitText(cell.innerText || cell.textContent || '', 120))
      ).filter((row) => row.length > 0)
    }));

  // HTML void elements: the HTML fragment serialization algorithm gives
  // these no end tag and no children, regardless of what the DOM holds.
  const VOID_ELEMENTS = new Set([
    'area', 'base', 'basefont', 'bgsound', 'br', 'col', 'embed', 'frame',
    'hr', 'img', 'input', 'keygen', 'link', 'meta', 'param', 'source',
    'track', 'wbr'
  ]);

  // Elements whose text children the serialization algorithm emits
  // verbatim (unescaped), matching outerHTML. `noscript` is spec-listed
  // too, but is handled separately below — its content is dropped
  // entirely rather than classified as raw-or-escaped — so it does not
  // need an entry here; see the `tag === 'noscript'` branch for why.
  const RAW_TEXT_PARENTS = new Set([
    'style', 'script', 'xmp', 'iframe', 'noembed', 'noframes', 'plaintext'
  ]);

  const ESCAPES = { '&': '&amp;', '\u00a0': '&nbsp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' };
  const escapeChar = (ch) => ESCAPES[ch];
  // Text nodes: & < > nbsp. Attribute values: & " nbsp, but not < or > —
  // this engine's own outerHTML leaves those two literal in attributes.
  const escapeText = (data) => String(data).replace(/[&\u00a0<>]/g, escapeChar);
  const escapeAttrValue = (data) => String(data).replace(/[&\u00a0"]/g, escapeChar);

  const isSecretInput = (element) => {
    if (element.localName !== 'input') {
      return false;
    }
    const type = ((element.getAttribute && element.getAttribute('type')) || '').toLowerCase();
    return isSecretInputType(type);
  };

  // Read-only live-tree walk, no clone, no HTML parse; drop iframe[srcdoc] and noscript children because escaping does not redact them.
  const serializeSanitizedHtml = (root, max) => {
    if (!root) {
      return null;
    }

    const parts = [];
    let length = 0;
    const emit = (str) => {
      parts.push(str);
      length += str.length;
    };

    // Each stack frame is either a node still to be visited (`open`) or an
    // end tag to emit once that node's children are done (`close`); that
    // pairing is what lets one iterative loop reproduce the same
    // open-tag / children / end-tag order a recursive walk would produce.
    const stack = [{ open: root }];

    while (stack.length > 0 && length < max) {
      const frame = stack.pop();

      if (frame.close) {
        emit(`</${frame.close}>`);
        continue;
      }

      const node = frame.open;
      switch (node.nodeType) {
        case 1: { // Element
          const tag = node.localName;
          const secretInput = isSecretInput(node);
          const isIframe = tag === 'iframe';

          let open = `<${tag}`;
          for (const attr of Array.from(node.attributes)) {
            if (secretInput && attr.name === 'value') {
              continue; // password/hidden value — see isSecretInputType
            }
            if (isIframe && attr.name === 'srcdoc') {
              continue; // opaque HTML string; omitted, see comment above
            }
            open += ` ${attr.name}="${escapeAttrValue(attr.value)}"`;
          }
          open += '>';
          emit(open);

          if (VOID_ELEMENTS.has(tag)) {
            continue;
          }

          stack.push({ close: tag });

          if (tag === 'noscript') {
            // Drop noscript children because escaping does not redact them.
            continue;
          }

          // A <template>'s children live in its own inert `content`
          // DocumentFragment, not in its own childNodes. Substituting
          // that fragment's children here, uniformly at every depth, is
          // what makes templates nested inside templates fall out for
          // free — no separate recursive helper needed.
          const kids = tag === 'template' && node.content ? node.content.childNodes : node.childNodes;
          for (let i = kids.length - 1; i >= 0; i -= 1) {
            stack.push({ open: kids[i] });
          }
          continue;
        }
        case 3: { // Text
          const parent = node.parentElement;
          const raw = parent && RAW_TEXT_PARENTS.has(parent.localName);
          emit(raw ? node.data : escapeText(node.data));
          continue;
        }
        case 8: // Comment
          emit(`<!--${node.data}-->`);
          continue;
        case 7: // ProcessingInstruction
          emit(`<?${node.target} ${node.data}?>`);
          continue;
        default:
          // Other node types (e.g. a doctype) do not occur as descendants
          // of an element root; nothing to serialize.
          continue;
      }
    }

    return limitText(parts.join(''), max);
  };

  // These stamps describe page evidence, not an integrity boundary against
  // hostile page JavaScript. Native Rust owns caller routing and URL provenance.
  const documentId = typeof crypto.randomUUID === 'function' ? crypto.randomUUID()
    : Array.from(crypto.getRandomValues(new Uint8Array(16)), (byte) => byte.toString(16).padStart(2, '0')).join('');
  let revision = 0;
  let observedUrl = location.href;
  let observedRuntimeId = null;
  const targetIds = new WeakMap();
  const targets = new Map();
  let nextTargetId = 0;
  let targetRevision = -1;
  const observer = new MutationObserver((records) => { if (records.length) revision += 1; });
  observer.observe(document, { subtree: true, childList: true, attributes: true, characterData: true });
  document.addEventListener('input', () => { revision += 1; }, true);
  document.addEventListener('change', () => { revision += 1; }, true);
  const syncRevision = () => {
    if (observer.takeRecords().length) revision += 1;
    if (location.href !== observedUrl) { observedUrl = location.href; revision += 1; }
    // Re-observation cannot silently replace the approval's private fingerprint
    // after a property-only field edit that emits no mutation/input event.
    if (Array.from(targets.values()).some((target) => !target.element.isConnected
        || target.element.ownerDocument !== document || target.fingerprint !== targetFingerprint(target.element))) revision += 1;
    if (targetRevision !== revision) { targets.clear(); targetRevision = revision; }
  };
  const byteLimit = (value, max) => {
    let result = '', bytes = 0;
    for (const char of String(value || '').trim()) {
      const cp = char.codePointAt(0);
      const size = cp <= 0x7f ? 1 : cp <= 0x7ff ? 2 : cp <= 0xffff ? 3 : 4;
      if (bytes + size > max) break;
      result += char; bytes += size;
    }
    return result;
  };
  const excludedEvidence = (node) => {
    for (let element = node.nodeType === 1 ? node : node.parentElement; element; element = element.parentElement) {
      if (['INPUT', 'TEXTAREA', 'SELECT', 'SCRIPT', 'STYLE', 'TEMPLATE', 'NOSCRIPT'].includes(element.tagName)
          || element.hidden || element.getAttribute('aria-hidden') === 'true') return true;
    }
    return false;
  };
  const evidenceText = (root, max, omissions) => {
    if (!root || excludedEvidence(root)) return '';
    const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
    let text = '', node, visited = 0;
    while ((node = walker.nextNode())) {
      visited += 1;
      if (visited > 10000) { omissions?.add('Text traversal limit reached'); break; }
      if (excludedEvidence(node)) continue;
      const addition = (text ? ' ' : '') + (node.textContent || '').trim();
      const bounded = byteLimit(text + addition, max);
      if (bounded !== (text + addition).trim()) { omissions?.add('Page text truncated'); text = bounded; break; }
      text = bounded;
    }
    return text;
  };
  const targetLabel = (element) => byteLimit(
    element.getAttribute('aria-label') ||
    Array.from(element.labels || []).map((label) => evidenceText(label, 240)).join(' ') ||
    element.getAttribute('alt') || evidenceText(element, 240) || element.getAttribute('name') || '', 240);
  const isSubmitControl = (element) => ['BUTTON', 'INPUT'].includes(element.tagName)
    && ['submit', 'image'].includes(element.type) && !!element.form;
  const targetDestination = (element) => {
    if (element.matches('a[href]')) return element.href;
    const form = element.tagName === 'FORM' ? element : element.form || element.closest('form');
    if (!form) return null;
    return isSubmitControl(element) && element.hasAttribute('formaction') ? element.formAction : form.action;
  };
  const targetFingerprint = (element) => {
    const form = element.tagName === 'FORM' ? element : element.form || element.closest('form');
    // Values are compared only inside the runtime and never serialized as evidence.
    return JSON.stringify([element.tagName, element.id, targetLabel(element), element.type,
      !!element.disabled, !!element.readOnly, element.getAttribute('href'),
      element.getAttribute('formaction'), targetDestination(element), form?.method,
      'value' in element ? element.value : null,
      form ? Array.from(form.elements).slice(0, 500).map((input) =>
        [input.name, input.type, input.value, input.checked, input.disabled]) : null]);
  };
  // Attribute checks miss CSS-hidden and inert subtrees; require a rendered, actionable element.
  const isRendered = (element) => {
    if (element.closest('[hidden], [aria-hidden="true"], [inert]')) return false;
    if (typeof element.checkVisibility === 'function') {
      return element.checkVisibility({ checkVisibilityCSS: true, visibilityProperty: true });
    }
    // Layout-free fallback: no box geometry, so walk ancestors for display:none.
    if (getComputedStyle(element).visibility === 'hidden') return false;
    for (let node = element; node; node = node.parentElement)
      if (getComputedStyle(node).display === 'none') return false;
    return true;
  };
  // Assigning `.value` directly hits React's instrumented setter and its value tracker
  // then swallows the input event; the prototype setter keeps onChange firing.
  const setNativeValue = (element, value) => {
    const proto = element.tagName === 'TEXTAREA' ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    const setter = Object.getOwnPropertyDescriptor(proto, 'value')?.set;
    if (setter) setter.call(element, value); else element.value = value;
  };
  const observePage = (runtimeId, requested = {}) => {
    const ceilings = { max_text_bytes: 12000, max_targets: 80, max_links: 40,
      max_tables: 6, max_rows: 20, max_cell_bytes: 240 };
    const limits = Object.fromEntries(Object.entries(ceilings).map(([key, ceiling]) =>
      [key, Number.isSafeInteger(requested[key]) && requested[key] >= 0 ? Math.min(requested[key], ceiling) : ceiling]));
    syncRevision();
    observedRuntimeId = runtimeId;
    const omissions = new Set(['Raw HTML, input values, frames and visual evidence are excluded']);
    const candidates = document.querySelectorAll('a[href], button, input:not([type="hidden"]), textarea, select, form, [role="button"], [role="link"]');
    const observedTargets = [];
    for (const element of candidates) {
      if (!isRendered(element)) continue;
      const destination = targetDestination(element);
      if (destination !== null && byteLimit(destination, 4096) !== destination) { omissions.add('Oversized destination targets excluded'); continue; }
      if (observedTargets.length >= limits.max_targets) { omissions.add('Targets truncated'); break; }
      let id = targetIds.get(element);
      if (!id) { id = `target-${++nextTargetId}`; targetIds.set(element, id); }
      targets.set(id, { element, fingerprint: targetFingerprint(element) });
      observedTargets.push({ id, role: byteLimit(element.getAttribute('role') || element.tagName.toLowerCase(), 80),
        tag: element.tagName.toLowerCase(), label: targetLabel(element), disabled: !!element.disabled,
        sensitive: element.type === 'password', destination });
    }
    const anchors = document.querySelectorAll('a[href]');
    if (anchors.length > limits.max_links) omissions.add('Links truncated');
    const links = [];
    for (const link of anchors) {
      if (links.length >= limits.max_links) break;
      if (byteLimit(link.href, 4096) !== link.href) { omissions.add('Oversized link URLs excluded'); continue; }
      links.push({ href: link.href, text: evidenceText(link, 240, omissions) });
    }
    const tableNodes = document.querySelectorAll('table');
    if (tableNodes.length > limits.max_tables) omissions.add('Tables truncated');
    const tables = Array.from(tableNodes).slice(0, limits.max_tables).map((table) => {
      const headerNodes = table.querySelectorAll('th');
      const rowNodes = Array.from(table.querySelectorAll('tr')).filter((row) => row.querySelector('td'));
      if (headerNodes.length > 20 || rowNodes.length > limits.max_rows) omissions.add('Table cells or rows truncated');
      return { headers: Array.from(headerNodes).slice(0, 20).map((cell) => evidenceText(cell, limits.max_cell_bytes, omissions)),
        rows: Array.from(rowNodes).slice(0, limits.max_rows).map((row) => {
          const cells = row.querySelectorAll('td');
          if (cells.length > 20) omissions.add('Table cells or rows truncated');
          return Array.from(cells).slice(0, 20).map((cell) => evidenceText(cell, limits.max_cell_bytes, omissions));
        }).filter((row) => row.length) };
    });
    const text = evidenceText(document.body, limits.max_text_bytes, omissions);
    return { schema_version: 1, document: { runtime_id: runtimeId, document_id: documentId, revision },
      url: location.href, title: byteLimit(document.title, 512), text, links, tables, targets: observedTargets,
      omissions: [...omissions], capabilities: { schema_version: 1, runtime: 'desktop_webview', javascript: true,
        interaction: true, scoped_targets: true, native_url: true, screenshots: false,
        background_javascript: false, enforcing_subresource_network: false }, collected_at_ms: Date.now() };
  };
  const dispatchTarget = (command) => {
    const reject = (message) => ({ state: 'not_dispatched', message });
    syncRevision();
    if (!command?.document || command.document.runtime_id !== observedRuntimeId
        || command.document.document_id !== documentId || command.document.revision !== revision)
      return reject('Reviewed document state is stale; observe again');
    const target = targets.get(command.target_id);
    if (!target || !target.element.isConnected || target.element.ownerDocument !== document
        || target.fingerprint !== targetFingerprint(target.element)) return reject('Reviewed target is stale; observe again');
    const element = target.element;
    if (!isRendered(element)) return reject('Target is not rendered; observe again');
    if (element.disabled) return reject('Target is disabled');
    if (!['click', 'type', 'submit', 'scroll'].includes(command.action)) return reject('Unsupported target action');
    if (command.action === 'type' && (!['INPUT', 'TEXTAREA'].includes(element.tagName)
        || element.readOnly || typeof command.text !== 'string')) return reject('Target cannot accept typed text');
    const form = element.tagName === 'FORM' ? element : element.form || element.closest('form');
    if (form && form.elements.length > 500) return reject('Form exceeds the bounded freshness validation limit');
    if (command.action === 'submit' && (!form || !(element.tagName === 'FORM' || isSubmitControl(element))))
      return reject('Submit target must be a form or its submit control');
    if (command.action === 'submit' && typeof form.requestSubmit !== 'function'
        && isSubmitControl(element) && element.hasAttribute('formaction'))
      return reject('Runtime cannot submit this overridden destination with its reviewed submitter');
    // All validation and dispatch occur in this synchronous turn. A throwing
    // page handler after dispatch creates uncertainty, never a safe retry.
    try {
      if (command.action === 'click') element.click();
      if (command.action === 'type') {
        element.focus(); setNativeValue(element, command.text);
        element.dispatchEvent(new Event('input', { bubbles: true }));
        element.dispatchEvent(new Event('change', { bubbles: true }));
      }
      if (command.action === 'submit') {
        if (typeof form.requestSubmit === 'function') {
          if (isSubmitControl(element)) form.requestSubmit(element); else form.requestSubmit();
        } else if (isSubmitControl(element)) element.click(); else form.submit();
      }
      if (command.action === 'scroll') element.scrollIntoView({ behavior: 'auto', block: 'center', inline: 'center' });
      revision += 1;
      return { state: 'acknowledged', message: 'Action dispatch acknowledged; outcome has not been verified' };
    } catch (_) {
      revision += 1;
      return { state: 'unknown', message: 'Action dispatch may have occurred; inspect the page before continuing' };
    }
  };

  const runtime = {
    observe: observePage,
    dispatchTarget,
    dispatch(pageId, requestId, producer) {
      if (!invoke) {
        throw new Error('Tauri invoke bridge is not available in this webview');
      }

      Promise.resolve()
        .then(producer)
        .then((payload) => invoke('browser_runtime_report', {
          payload: {
            page_id: pageId,
            request_id: requestId,
            payload,
            error: null
          }
        }))
        .catch((error) => {
          const message = error && error.message ? error.message : String(error);
          return invoke('browser_runtime_report', {
            payload: {
              page_id: pageId,
              request_id: requestId,
              payload: null,
              error: message
            }
          });
        });
    },

    snapshot() {
      const root = document.documentElement;
      return {
        url: window.location.href,
        title: document.title || '',
        html: serializeSanitizedHtml(root, 250000),
        text: document.body ? limitText(document.body.innerText || document.body.textContent || '', 50000) : null,
        viewport_width: window.innerWidth || 0,
        viewport_height: window.innerHeight || 0,
        scroll_x: window.scrollX || 0,
        scroll_y: window.scrollY || 0,
        interactive_ready: document.readyState === 'interactive' || document.readyState === 'complete',
        links: Array.from(document.querySelectorAll('a[href]')).slice(0, 200).map((link) => ({
          href: link.href || '',
          text: limitText(link.innerText || link.textContent || '', 160)
        })),
        images: Array.from(document.querySelectorAll('img')).slice(0, 200).map((image) => ({
          src: image.currentSrc || image.src || '',
          alt: image.alt || '',
          width: Number.isFinite(image.naturalWidth) && image.naturalWidth > 0 ? image.naturalWidth : null,
          height: Number.isFinite(image.naturalHeight) && image.naturalHeight > 0 ? image.naturalHeight : null
        })),
        forms: collectForms(),
        prices: [],
        tables: collectTables()
      };
    },

    querySelector(selector) {
      return Array.from(document.querySelectorAll(selector)).slice(0, 100).map((element) => serializeElement(element, selector));
    },

    getText(selector) {
      return Array.from(document.querySelectorAll(selector))
        .slice(0, 100)
        .map((element) => limitText(element.innerText || element.textContent || '', 1000))
        .join('\n');
    },

    click(selector) {
      const element = document.querySelector(selector);
      if (!element) {
        throw new Error(`No element matched selector: ${selector}`);
      }
      element.click();
      return { ok: true };
    },

    typeText(selector, text) {
      const element = document.querySelector(selector);
      if (!element) {
        throw new Error(`No element matched selector: ${selector}`);
      }
      if (!('value' in element)) {
        throw new Error(`Element does not support value assignment: ${selector}`);
      }
      element.focus();
      setNativeValue(element, text);
      element.dispatchEvent(new Event('input', { bubbles: true }));
      element.dispatchEvent(new Event('change', { bubbles: true }));
      return { ok: true };
    },

    submitForm(selector) {
      const target = document.querySelector(selector);
      if (!target) {
        throw new Error(`No element matched selector: ${selector}`);
      }
      const form = target.tagName && target.tagName.toLowerCase() === 'form'
        ? target
        : target.closest('form');
      if (!form) {
        throw new Error(`No form found for selector: ${selector}`);
      }
      if (typeof form.requestSubmit === 'function') {
        form.requestSubmit();
      } else {
        form.submit();
      }
      return { ok: true };
    },

    scrollTo(selector) {
      const element = document.querySelector(selector);
      if (!element) {
        throw new Error(`No element matched selector: ${selector}`);
      }
      element.scrollIntoView({ behavior: 'auto', block: 'center', inline: 'center' });
      return { ok: true };
    },

    scrollBy(x, y) {
      window.scrollBy(x, y);
      return { ok: true };
    },

    keypress(key) {
      const target = document.activeElement || document.body || document.documentElement;
      if (!target) {
        throw new Error('No focused element available for keypress');
      }
      const init = { key, bubbles: true, cancelable: true };
      target.dispatchEvent(new KeyboardEvent('keydown', init));
      target.dispatchEvent(new KeyboardEvent('keyup', init));
      return { ok: true };
    }
  };

  Object.defineProperty(window, '__NEUROBROWSER_RUNTIME__', {
    value: Object.freeze(runtime), writable: false, configurable: false
  });
})();
"#;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Default)]
pub struct BrowserViewport {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

enum NavigationPoll {
    Pending,
    Ready,
    Cancelled(String),
}

struct RuntimePage {
    runtime_id: String,
    viewport: BrowserViewport,
    loading: bool,
    /// Netguard refused a hop. Distinct from `loading == false`, which also
    /// means a load finished, so a waiter can tell cancel from success.
    navigation_cancel: Option<String>,
}

struct PendingRuntimeRequest {
    page_id: usize,
    runtime_id: String,
    sender: oneshot::Sender<Result<Value, String>>,
}

#[derive(Default)]
pub struct BrowserRuntimeRegistry {
    pages: Mutex<HashMap<usize, RuntimePage>>,
    pending: Mutex<HashMap<String, PendingRuntimeRequest>>,
    active_page: Mutex<Option<usize>>,
}

impl BrowserRuntimeRegistry {
    pub fn register_page(&self, page_id: usize, runtime_id: String) {
        self.pages.lock().unwrap().insert(
            page_id,
            RuntimePage {
                runtime_id,
                viewport: BrowserViewport::default(),
                loading: false,
                navigation_cancel: None,
            },
        );
    }

    pub fn unregister_page(&self, page_id: usize) -> Option<String> {
        let removed = self.pages.lock().unwrap().remove(&page_id);
        let mut active_page = self.active_page.lock().unwrap();
        if active_page.as_ref() == Some(&page_id) {
            *active_page = None;
        }
        let mut pending = self.pending.lock().unwrap();
        let request_ids: Vec<_> = pending
            .iter()
            .filter(|(_, request)| request.page_id == page_id)
            .map(|(id, _)| id.clone())
            .collect();
        for id in request_ids {
            if let Some(request) = pending.remove(&id) {
                let _ = request.sender.send(Err(
                    "Runtime page closed; action outcome may be unknown".into(),
                ));
            }
        }
        removed.map(|page| page.runtime_id)
    }

    pub fn page_runtime_id(&self, page_id: usize) -> Result<String, String> {
        self.pages
            .lock()
            .unwrap()
            .get(&page_id)
            .map(|page| page.runtime_id.clone())
            .ok_or_else(|| "Runtime page not found".to_string())
    }

    pub fn begin_request(
        &self,
        page_id: usize,
        runtime_id: &str,
    ) -> Result<(String, oneshot::Receiver<Result<Value, String>>), String> {
        // Hold page ownership until insertion so page closure cannot drain
        // requests and then race with a new orphaned pending request.
        let pages = self.pages.lock().unwrap();
        if pages
            .get(&page_id)
            .is_none_or(|page| page.runtime_id != runtime_id)
        {
            return Err("Runtime request ownership mismatch".into());
        }
        let request_id = uuid::Uuid::new_v4().to_string();
        let (sender, receiver) = oneshot::channel();
        self.pending.lock().unwrap().insert(
            request_id.clone(),
            PendingRuntimeRequest {
                page_id,
                runtime_id: runtime_id.to_string(),
                sender,
            },
        );
        Ok((request_id, receiver))
    }

    pub fn cancel_request(&self, request_id: &str) {
        self.pending.lock().unwrap().remove(request_id);
    }

    pub fn resolve_request(
        &self,
        page_id: usize,
        caller_runtime_id: &str,
        request_id: &str,
        payload: Option<Value>,
        error: Option<String>,
    ) -> Result<(), String> {
        if self.page_runtime_id(page_id)? != caller_runtime_id {
            return Err("Runtime report caller does not own this page".into());
        }
        let mut pending = self.pending.lock().unwrap();
        let request = pending
            .get(request_id)
            .ok_or("Runtime request is not pending")?;
        if request.page_id != page_id || request.runtime_id != caller_runtime_id {
            return Err("Runtime report does not own this pending request".into());
        }
        if let Some(request) = pending.remove(request_id) {
            let _ = request.sender.send(match error {
                Some(message) => Err(message),
                None => Ok(payload.unwrap_or(Value::Null)),
            });
        }
        Ok(())
    }

    pub fn set_loading(&self, page_id: usize, loading: bool) {
        if let Some(page) = self.pages.lock().unwrap().get_mut(&page_id) {
            page.loading = loading;
        }
    }

    /// A finished load clears `loading` and leaves any cancellation recorded.
    pub fn finish_loading(&self, page_id: usize) {
        self.set_loading(page_id, false);
    }

    /// A refused hop is not a finished load. `loading` is cleared so the page
    /// is not stuck, and the reason stays until a waiter or a new command reads it.
    pub fn cancel_navigation(&self, page_id: usize, reason: impl Into<String>) {
        if let Some(page) = self.pages.lock().unwrap().get_mut(&page_id) {
            page.loading = false;
            page.navigation_cancel = Some(reason.into());
        }
    }

    pub fn clear_navigation_cancel(&self, page_id: usize) {
        if let Some(page) = self.pages.lock().unwrap().get_mut(&page_id) {
            page.navigation_cancel = None;
        }
    }

    /// One lock, cancellation first. A cancel that also clears `loading` must
    /// not look like [`NavigationPoll::Ready`].
    fn poll_navigation(&self, page_id: usize) -> Result<NavigationPoll, String> {
        let mut pages = self.pages.lock().unwrap();
        let page = pages
            .get_mut(&page_id)
            .ok_or_else(|| "Runtime page not found".to_string())?;
        if let Some(reason) = page.navigation_cancel.take() {
            return Ok(NavigationPoll::Cancelled(reason));
        }
        if page.loading {
            Ok(NavigationPoll::Pending)
        } else {
            Ok(NavigationPoll::Ready)
        }
    }

    pub fn set_viewport(&self, page_id: usize, viewport: BrowserViewport) -> Result<(), String> {
        let mut pages = self.pages.lock().unwrap();
        let page = pages
            .get_mut(&page_id)
            .ok_or_else(|| "Runtime page not found".to_string())?;
        page.viewport = viewport;
        Ok(())
    }

    pub fn viewport(&self, page_id: usize) -> Result<BrowserViewport, String> {
        self.pages
            .lock()
            .unwrap()
            .get(&page_id)
            .map(|page| page.viewport)
            .ok_or_else(|| "Runtime page not found".to_string())
    }

    pub fn set_active_page(&self, page_id: Option<usize>) {
        *self.active_page.lock().unwrap() = page_id;
    }

    pub fn active_page(&self) -> Option<usize> {
        *self.active_page.lock().unwrap()
    }

    pub fn all_pages(&self) -> Vec<(usize, String)> {
        self.pages
            .lock()
            .unwrap()
            .iter()
            .map(|(page_id, page)| (*page_id, page.runtime_id.clone()))
            .collect()
    }
}

#[derive(Deserialize)]
pub struct RuntimeReportPayload {
    pub page_id: usize,
    pub request_id: String,
    pub payload: Option<Value>,
    pub error: Option<String>,
}

#[derive(Deserialize)]
struct TargetAcknowledgment {
    state: DispatchState,
    message: String,
}

#[derive(Clone)]
pub struct TauriBrowserRuntime {
    app: AppHandle,
    page_id: usize,
    runtime_id: String,
    registry: Arc<BrowserRuntimeRegistry>,
}

impl TauriBrowserRuntime {
    pub fn new(
        app: AppHandle,
        page_id: usize,
        runtime_id: String,
        registry: Arc<BrowserRuntimeRegistry>,
    ) -> Self {
        Self {
            app,
            page_id,
            runtime_id,
            registry,
        }
    }

    fn webview(&self) -> Result<Webview, String> {
        self.app
            .get_webview(&self.runtime_id)
            .ok_or_else(|| format!("Webview '{}' not found", self.runtime_id))
    }

    async fn request_json<T>(
        &self,
        script_expression: &str,
        action_request: bool,
    ) -> Result<T, String>
    where
        T: DeserializeOwned,
    {
        let webview = self.webview()?;
        let (request_id, receiver) = self
            .registry
            .begin_request(self.page_id, &self.runtime_id)?;
        let request_id_js = serde_json::to_string(&request_id).map_err(|e| e.to_string())?;
        let script = format!(
            "(() => {{ const runtime = window.__NEUROBROWSER_RUNTIME__; if (!runtime) {{ throw new Error('NeuroBrowser runtime bridge is not installed'); }} runtime.dispatch({}, {}, () => ({})); }})();",
            self.page_id, request_id_js, script_expression
        );

        if let Err(error) = webview.eval(script) {
            self.registry.cancel_request(&request_id);
            return Err(error.to_string());
        }

        let value = match tokio::time::timeout(REQUEST_TIMEOUT, receiver).await {
            Ok(result) => result.map_err(|_| {
                missing_runtime_response("Browser runtime response channel closed", action_request)
            })??,
            Err(_) => {
                self.registry.cancel_request(&request_id);
                return Err(missing_runtime_response(
                    &format!(
                        "Timed out waiting for browser runtime response for page {}",
                        self.page_id
                    ),
                    action_request,
                ));
            }
        };

        serde_json::from_value(value).map_err(|e| e.to_string())
    }

    async fn execute_action(&self, script_expression: &str) -> Result<(), String> {
        let _: Value = self.request_json(script_expression, true).await?;
        sleep(Duration::from_millis(120)).await;
        Ok(())
    }

    pub async fn wait_for_ready(&self, timeout_ms: u64) -> Result<(), String> {
        wait_for_page_ready(&self.registry, self.page_id, timeout_ms).await
    }
}

async fn wait_for_page_ready(
    registry: &BrowserRuntimeRegistry,
    page_id: usize,
    timeout_ms: u64,
) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    while Instant::now() <= deadline {
        match registry.poll_navigation(page_id)? {
            NavigationPoll::Cancelled(reason) => return Err(reason),
            NavigationPoll::Ready => return Ok(()),
            NavigationPoll::Pending => {}
        }
        sleep(Duration::from_millis(40)).await;
    }
    Err(format!(
        "Timed out waiting for page {page_id} to finish loading"
    ))
}

/// How long a navigation may take before the runtime reports it did not complete.
/// Generous enough for an ordinary slow page; short enough that a wedged load surfaces
/// as an error the agent can act on rather than an indefinite hang.
const NAVIGATION_READY_TIMEOUT_MS: u64 = 10_000;

fn missing_runtime_response(error: &str, action_request: bool) -> String {
    if action_request {
        format!(
            "Action outcome is unknown: {error}. The action may have executed. \
             Do not repeat it automatically; inspect the page before continuing."
        )
    } else {
        error.to_string()
    }
}

#[cfg(test)]
mod response_tests {
    use super::missing_runtime_response;

    #[test]
    fn incomplete_action_acknowledgments_cannot_be_treated_as_dispatch_success() {
        for payload in [
            serde_json::Value::Null,
            serde_json::json!({"ok": true}),
            serde_json::json!({"state": "acknowledged"}),
            serde_json::json!({"message": "claimed success"}),
            serde_json::json!({"state": "unrecognized", "message": "claimed success"}),
        ] {
            assert!(serde_json::from_value::<super::TargetAcknowledgment>(payload).is_err());
        }
        let acknowledgment: super::TargetAcknowledgment = serde_json::from_value(
            serde_json::json!({"state": "acknowledged", "message": "dispatch acknowledged"}),
        )
        .unwrap();
        assert_eq!(
            acknowledgment.state,
            neurobrowser::capability::DispatchState::Acknowledged
        );
    }

    #[test]
    fn report_ownership_mismatch_does_not_consume_pending_request() {
        let registry = super::BrowserRuntimeRegistry::default();
        registry.register_page(1, "page-runtime-1".into());
        registry.register_page(2, "page-runtime-2".into());
        let (id, mut receiver) = registry.begin_request(1, "page-runtime-1").unwrap();
        assert!(registry
            .resolve_request(2, "page-runtime-2", &id, None, None)
            .is_err());
        assert!(registry
            .resolve_request(1, "page-runtime-2", &id, None, None)
            .is_err());
        assert!(matches!(
            receiver.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ));
        registry
            .resolve_request(
                1,
                "page-runtime-1",
                &id,
                Some(serde_json::json!("correct")),
                None,
            )
            .unwrap();
        assert_eq!(
            receiver.try_recv().unwrap().unwrap(),
            serde_json::json!("correct")
        );
        assert!(registry
            .resolve_request(1, "page-runtime-1", &id, None, None)
            .is_err());
    }

    #[test]
    fn closing_page_drains_only_its_pending_requests() {
        let registry = super::BrowserRuntimeRegistry::default();
        registry.register_page(1, "page-runtime-1".into());
        registry.register_page(2, "page-runtime-2".into());
        let (_, mut first) = registry.begin_request(1, "page-runtime-1").unwrap();
        let (second_id, mut second) = registry.begin_request(2, "page-runtime-2").unwrap();
        registry.unregister_page(1);
        assert!(first.try_recv().unwrap().unwrap_err().contains("closed"));
        assert!(matches!(
            second.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ));
        registry
            .resolve_request(
                2,
                "page-runtime-2",
                &second_id,
                Some(serde_json::json!(true)),
                None,
            )
            .unwrap();
        assert_eq!(second.try_recv().unwrap().unwrap(), serde_json::json!(true));
        assert!(registry.begin_request(1, "page-runtime-1").is_err());
    }

    #[test]
    fn missing_action_acknowledgment_warns_of_unknown_outcome() {
        for error in [
            "Timed out waiting for browser runtime response for page 1",
            "Browser runtime response channel closed",
        ] {
            let message = missing_runtime_response(error, true);
            assert!(message.contains("Action outcome is unknown"));
            assert!(message.contains("may have executed"));
            assert!(message.contains("Do not repeat it automatically"));
            assert!(message.contains(error));
            assert!(!message.contains("Action executed,"));
            assert_eq!(missing_runtime_response(error, false), error);
        }
    }

    #[test]
    fn cancelled_navigation_wait_returns_error_instead_of_success() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap();
        runtime.block_on(async {
            let registry = super::BrowserRuntimeRegistry::default();
            registry.register_page(4, "page-runtime-4".into());
            registry.set_loading(4, true);
            registry
                .cancel_navigation(4, "Refusing to navigate to disallowed scheme 'javascript:'");
            // Finish must not erase the cancellation and make the wait succeed.
            registry.finish_loading(4);
            let error = super::wait_for_page_ready(&registry, 4, 500)
                .await
                .expect_err("cancelled hop");
            assert!(
                error.contains("disallowed scheme"),
                "unexpected wait error: {error}"
            );

            registry.clear_navigation_cancel(4);
            registry.set_loading(4, true);
            registry.finish_loading(4);
            super::wait_for_page_ready(&registry, 4, 500)
                .await
                .expect("finished load");
        });
    }
}

#[async_trait]
impl BrowserInterface for TauriBrowserRuntime {
    fn capabilities(&self) -> RuntimeCapabilities {
        RuntimeCapabilities::desktop()
    }

    async fn observe(&self, limits: ObservationLimits) -> Result<PageObservation, String> {
        let before_url = self
            .webview()?
            .url()
            .map_err(|error| error.to_string())?
            .to_string();
        let limits = limits.bounded();
        let limits_json = serde_json::to_string(&limits).map_err(|error| error.to_string())?;
        let runtime_id_json =
            serde_json::to_string(&self.runtime_id).map_err(|error| error.to_string())?;
        let mut observation: PageObservation = self
            .request_json(
                &format!("runtime.observe({runtime_id_json}, {limits_json})"),
                false,
            )
            .await?;
        let native_url = self
            .webview()?
            .url()
            .map_err(|error| error.to_string())?
            .to_string();
        if before_url != native_url {
            return Err("Page navigated while collecting evidence; observe again".into());
        }
        if observation
            .document
            .as_ref()
            .is_none_or(|document| document.runtime_id != self.runtime_id)
        {
            return Err("Observation runtime identity mismatch".into());
        }
        observation.url = native_url;
        observation.capabilities = self.capabilities();
        observation.apply_limits(limits);
        observation.validate()?;
        Ok(observation)
    }

    async fn dispatch_target(&self, command: &TargetCommand) -> Result<(), TargetDispatchError> {
        if command.document.runtime_id != self.runtime_id {
            return Err(TargetDispatchError::rejected(
                "Target belongs to a different runtime",
            ));
        }
        let command_json = serde_json::to_string(command)
            .map_err(|error| TargetDispatchError::rejected(error.to_string()))?;
        let result: TargetAcknowledgment = self
            .request_json(&format!("runtime.dispatchTarget({command_json})"), true)
            .await
            .map_err(TargetDispatchError::unknown)?;
        match result.state {
            DispatchState::Acknowledged => Ok(()),
            DispatchState::NotDispatched => Err(TargetDispatchError::rejected(result.message)),
            DispatchState::Unknown => Err(TargetDispatchError::unknown(result.message)),
        }
    }

    async fn navigate(&self, url: &str) -> Result<(), String> {
        if let Some(reason) = neurobrowser::netguard::blocked_reason(url) {
            tracing::warn!("Blocked navigation to {}: {}", url, reason);
            return Err(reason.to_string());
        }
        let parsed = url.parse::<tauri::Url>().map_err(|e| e.to_string())?;
        self.registry.clear_navigation_cancel(self.page_id);
        self.registry.set_loading(self.page_id, true);
        self.webview()?
            .navigate(parsed)
            .map_err(|e| e.to_string())?;
        self.wait_for_navigation().await
    }

    async fn query_selector(&self, selector: &str) -> Result<Vec<ElementInfo>, String> {
        let selector_json = serde_json::to_string(selector).map_err(|e| e.to_string())?;
        self.request_json(&format!("runtime.querySelector({selector_json})"), false)
            .await
    }

    async fn get_text(&self, selector: &str) -> Result<String, String> {
        let selector_json = serde_json::to_string(selector).map_err(|e| e.to_string())?;
        self.request_json(&format!("runtime.getText({selector_json})"), false)
            .await
    }

    async fn click(&self, selector: &str) -> Result<(), String> {
        let selector_json = serde_json::to_string(selector).map_err(|e| e.to_string())?;
        self.execute_action(&format!("runtime.click({selector_json})"))
            .await
    }

    async fn type_text(&self, selector: &str, text: &str) -> Result<(), String> {
        let selector_json = serde_json::to_string(selector).map_err(|e| e.to_string())?;
        let text_json = serde_json::to_string(text).map_err(|e| e.to_string())?;
        self.execute_action(&format!("runtime.typeText({selector_json}, {text_json})"))
            .await
    }

    async fn submit_form(&self, selector: &str) -> Result<(), String> {
        let selector_json = serde_json::to_string(selector).map_err(|e| e.to_string())?;
        self.execute_action(&format!("runtime.submitForm({selector_json})"))
            .await
    }

    async fn scroll_to(&self, selector: &str) -> Result<(), String> {
        let selector_json = serde_json::to_string(selector).map_err(|e| e.to_string())?;
        self.execute_action(&format!("runtime.scrollTo({selector_json})"))
            .await
    }

    async fn scroll_by(&self, x: f32, y: f32) -> Result<(), String> {
        self.execute_action(&format!("runtime.scrollBy({}, {})", x, y))
            .await
    }

    async fn keypress(&self, key: &str) -> Result<(), String> {
        let key_json = serde_json::to_string(key).map_err(|e| e.to_string())?;
        self.execute_action(&format!("runtime.keypress({key_json})"))
            .await
    }

    async fn browser_back(&self) -> Result<(), String> {
        // A missing history entry emits no load event. Let real page-load
        // callbacks own loading, as for click/submit, rather than inventing one.
        self.registry.clear_navigation_cancel(self.page_id);
        self.execute_action("history.back()").await?;
        self.wait_for_navigation().await
    }

    async fn browser_forward(&self) -> Result<(), String> {
        // A missing history entry emits no load event. Let real page-load
        // callbacks own loading, as for click/submit, rather than inventing one.
        self.registry.clear_navigation_cancel(self.page_id);
        self.execute_action("history.forward()").await?;
        self.wait_for_navigation().await
    }

    async fn browser_reload(&self) -> Result<(), String> {
        self.registry.clear_navigation_cancel(self.page_id);
        self.registry.set_loading(self.page_id, true);
        self.webview()?.reload().map_err(|e| e.to_string())
    }

    async fn snapshot(&self) -> Result<PageSnapshot, String> {
        let before_url = self
            .webview()?
            .url()
            .map_err(|error| error.to_string())?
            .to_string();
        let mut snapshot: PageSnapshot = self.request_json("runtime.snapshot()", false).await?;
        snapshot.url = self
            .webview()?
            .url()
            .map_err(|error| error.to_string())?
            .to_string();
        if snapshot.url != before_url {
            return Err("Page navigated while collecting snapshot; observe again".into());
        }
        enrich_snapshot(&mut snapshot);
        Ok(snapshot)
    }

    async fn wait_for_navigation(&self) -> Result<(), String> {
        self.wait_for_ready(NAVIGATION_READY_TIMEOUT_MS).await
    }
}

pub fn create_runtime_page(
    window: &Window,
    registry: Arc<BrowserRuntimeRegistry>,
    page_id: usize,
    runtime_id: &str,
) -> Result<(), String> {
    let external_url = ABOUT_BLANK_URL
        .parse::<tauri::Url>()
        .map_err(|e| e.to_string())?;
    let registry_for_nav = registry.clone();
    let registry_for_load = registry.clone();

    let builder = WebviewBuilder::new(runtime_id, WebviewUrl::External(external_url))
        .initialization_script(RUNTIME_INIT_SCRIPT)
        // Cancel hops the shared netguard rejects (redirects, JS, clicked links).
        .on_navigation(move |url| {
            if let Some(reason) = neurobrowser::netguard::blocked_reason(url.as_str()) {
                tracing::warn!("Blocked webview navigation to {}: {}", url, reason);
                registry_for_nav.cancel_navigation(page_id, reason.to_string());
                return false;
            }
            registry_for_nav.set_loading(page_id, true);
            true
        })
        .on_page_load(move |_webview, payload| match payload.event() {
            PageLoadEvent::Started => registry_for_load.set_loading(page_id, true),
            PageLoadEvent::Finished => registry_for_load.finish_loading(page_id),
        });

    let webview = window
        .add_child(
            builder,
            LogicalPosition::new(0.0, 0.0),
            LogicalSize::new(1.0, 1.0),
        )
        .map_err(|e| e.to_string())?;

    webview.set_auto_resize(false).map_err(|e| e.to_string())?;
    webview.hide().map_err(|e| e.to_string())?;
    registry.register_page(page_id, runtime_id.to_string());
    Ok(())
}

pub fn close_runtime_page(
    app: &AppHandle,
    registry: &BrowserRuntimeRegistry,
    page_id: usize,
) -> Result<(), String> {
    if let Some(runtime_id) = registry.unregister_page(page_id) {
        if let Some(webview) = app.get_webview(&runtime_id) {
            webview.close().map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

pub fn set_active_runtime_page(
    app: &AppHandle,
    registry: &BrowserRuntimeRegistry,
    page_id: usize,
) -> Result<(), String> {
    registry.set_active_page(Some(page_id));
    for (known_page_id, runtime_id) in registry.all_pages() {
        let webview = app
            .get_webview(&runtime_id)
            .ok_or_else(|| format!("Webview '{}' not found", runtime_id))?;
        if known_page_id == page_id {
            let viewport = registry.viewport(known_page_id)?;
            webview
                .set_position(LogicalPosition::new(viewport.x, viewport.y))
                .map_err(|e| e.to_string())?;
            webview
                .set_size(LogicalSize::new(
                    viewport.width.max(1.0),
                    viewport.height.max(1.0),
                ))
                .map_err(|e| e.to_string())?;
            webview.show().map_err(|e| e.to_string())?;
        } else {
            webview.hide().map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

pub fn sync_runtime_viewport(
    app: &AppHandle,
    registry: &BrowserRuntimeRegistry,
    page_id: usize,
    viewport: BrowserViewport,
) -> Result<(), String> {
    registry.set_viewport(page_id, viewport)?;
    if registry.active_page() == Some(page_id) {
        let runtime_id = registry.page_runtime_id(page_id)?;
        let webview = app
            .get_webview(&runtime_id)
            .ok_or_else(|| format!("Webview '{}' not found", runtime_id))?;
        webview
            .set_position(LogicalPosition::new(viewport.x, viewport.y))
            .map_err(|e| e.to_string())?;
        webview
            .set_size(LogicalSize::new(
                viewport.width.max(1.0),
                viewport.height.max(1.0),
            ))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}
