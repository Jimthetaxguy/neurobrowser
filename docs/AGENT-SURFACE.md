# NeuroBrowser — Agent Surface (spec-of-record)

This document is the canonical spec for the **agent-facing tool surface** that
any external agent (ROSA, Claude Code, a custom worker) uses to drive
NeuroBrowser. It is the source-of-truth for:

- The 12 tools (`snapshot`, `click`, `type_text`, `submit_form`,
  `query_selector`, `evaluate`, `navigate`, `get_text`, `get_attribute`,
  `wait_for`, `extract_text`, `screenshot`).
- The JSON schemas for each tool's arguments and return values.
- The three autonomy levels (`ReadOnly`, `Assisted`, `HighAutonomy`) and the
  policy gates that bind them.

`SKILL.md` mirrors this document as the agent-loadable invocation spec. Update
both together.

## Concept: ref-based interaction

External agents pass `[@e1, @e2, ...]` **element refs** rather than CSS
selectors or XPaths. Every `snapshot()` returns a ref-map alongside the
accessibility tree:

```json
{
  "url": "https://example.com/login",
  "title": "Sign in - Example",
  "viewport": { "width": 1280, "height": 720, "scroll_x": 0, "scroll_y": 0 },
  "ref_map": {
    "@e1": { "tag": "input", "id": "email",    "classes": ["field"],     "xpath": "//input[@id='email']" },
    "@e2": { "tag": "input", "id": "password", "classes": ["field"],     "xpath": "//input[@id='password']" },
    "@e3": { "tag": "button","id": "submit",  "classes": ["primary"],   "xpath": "//button[@id='submit']" }
  },
  "tree": "<accessibility tree as flattened ARIA>"
}
```

Refs are stable for the lifetime of the current snapshot. After navigation,
re-snapshot and re-resolve refs. This avoids the brittle-text-selector problem
of CSS selectors (which break when a class name changes) and the
implementation-coupled problem of XPaths (which break when developers refactor
DOM).

## Tools (12)

### 1. `snapshot(url_or_ref)`

Take a snapshot of a page. If `url_or_ref` is a URL, navigate first then
snapshot. If it's a ref (e.g. `@e3`), snapshot without navigation.

```json
// arguments
{ "url_or_ref": "https://example.com/login" }

// return — matches the example above
{ "url", "title", "viewport", "ref_map", "tree" }
```

### 2. `click(ref)`

Click an element by ref. Returns before/after URL (useful for confirming
navigation).

```json
// arguments
{ "ref": "@e3" }

// return
{ "ok": true, "before_url": "https://example.com/login", "after_url": "https://example.com/dashboard" }
```

If `click` requires approval under the active policy, the return is:

```json
{ "ok": false, "pending_approval_id": "uuid", "reasons": ["Assisted mode requires confirmation for clicks"] }
```

### 3. `type_text(ref, text)`

Type text into an input by ref. Fires `InputEvent`s to the element.

```json
{ "ref": "@e2", "text": "correct horse battery staple" }
→ { "ok": true }
```

### 4. `submit_form(ref)`

Submit a form (or invoke a button) by ref. Waits for the resulting
navigation.

```json
{ "ref": "@e3" }
→ { "ok": true, "response_url": "https://example.com/dashboard" }
```

### 5. `query_selector(selector)`

Resolve a CSS selector to a list of refs. Useful when an agent knows the
selectors but hasn't snapshotted.

```json
{ "selector": "nav a" }
→ { "elements": ["@e11", "@e12", "@e13"] }
```

### 6. `evaluate(script)`

Run a JavaScript expression inside the webview's sandbox. Returns the value
as a string. Sandboxed to the page's origin; cross-origin reads blocked.

```json
{ "script": "document.cookie" }
→ { "value": "session=abc123" }
```

### 7. `navigate(url)`

Navigate the active page to a URL. Normalized (adds `https://` if missing).

```json
{ "url": "example.com/about" }
→ { "ok": true, "page_handle": "0" }
```

### 8. `get_text(ref)`

Read the visible text of an element.

```json
{ "ref": "@e1" }
→ { "text": "Email address" }
```

### 9. `get_attribute(ref, name)`

Read a single attribute of an element. Returns `null` if absent.

```json
{ "ref": "@e3", "name": "data-test-id" }
→ { "value": "submit-btn" }

// or for a missing attr
→ { "value": null }
```

### 10. `wait_for(selector, timeout_ms)`

Block until the selector matches at least one element, or timeout.

```json
{ "selector": ".dashboard", "timeout_ms": 8000 }
→ { "ok": true, "elapsed_ms": 1240 }
```

### 11. `extract_text(ref, structured?)`

Read the inner text and (optionally) parse it into a structured form.

```json
{ "ref": "@e20", "structured": true }
→ {
    "text": "Total: $42.50\nDate: 2026-07-08",
    "structured": { "total": "42.50", "currency": "USD", "date": "2026-07-08" }
  }
```

`structured=true` returns a best-effort parse based on common patterns
(amounts, dates, prices). For tables / lists / repeating structures, prefer
`snapshot` + the `tree` field.

### 12. `screenshot(viewport?)`

Take a PNG screenshot of the current page. Returns base64-encoded bytes.

```json
{ "viewport": { "width": 1280, "height": 720 } }
→ { "base64_png": "iVBORw0KGgo...", "viewport": { "width": 1280, "height": 720 }, "size_bytes": 42183 }
```

If `viewport` is omitted, uses the page's current viewport. Screenshots
are full-page by default; pass `viewport.fullPage = false` for viewport-only.

## Autonomy levels

Three levels bind to the `ActionPolicy`'s `AutonomyLevel`:

| Level | Allowed actions | Blocked actions |
|---|---|---|
| `ReadOnly` | `snapshot`, `query_selector`, `get_text`, `get_attribute`, `extract_text`, `screenshot`, `evaluate` (read-only scripts only) | `click`, `type_text`, `submit_form`, `navigate`, `evaluate` (anything that mutates) |
| `Assisted` | everything in `ReadOnly` + `click`, `type_text`, `submit_form`, `navigate` | actions that touch `denied_domains` or trigger `RiskFlag::Sensitive` require explicit approval (returned as `pending_approval_id`) |
| `HighAutonomy` | everything | actions touching `denied_domains` still block; sensitive-args (passwords, tokens) auto-redact but the call runs |

## Policy gates

Every tool call goes through `ActionPolicy::evaluate` before execution. The
policy inspects:

1. **Domain membership** — if the current page URL is in
   `ActionPolicy.denied_domains`, the call returns `Block` with
   `RiskFlag::DomainDenied`.
2. **Autonomy level** — if the tool is not allowed at the current level,
   the call returns `RequireApproval` (Assisted) or `Block` (ReadOnly).
3. **Sensitive-arg redaction** — argument values whose keys match
   `(password|token|secret|api_key|apikey|ssn|social|credit|card|cvv|otp|auth)`
   are replaced with `[REDACTED]` in the audit trail and events. The actual
   call still runs (with the real value) unless the policy is in a stricter
   mode (Phase E will add a per-worker redact-only mode).
4. **Prompt-injection detection** — substring detection on tool arguments;
   if the argument value contains `ignore previous instructions` or
   `reveal your instructions`, the call is `Block`-ed.

The current `ActionPolicy` is exposed via `set_action_policy` /
`get_action_policy` Tauri commands (and, in the headless daemon, via the
`policy.get` / `policy.set` IPC). Agents can read it and reason about it.

## Error shape

Tool calls that fail return:

```json
{ "ok": false, "error": { "code": "TIMEOUT", "message": "..." } }
```

Codes:

- `TIMEOUT` — exceeded `timeout_ms` (default 8s, 3s for follow-on actions).
- `BLOCKED` — call denied by policy.
- `NOT_FOUND` — ref or selector didn't resolve.
- `EVAL_ERROR` — `evaluate` script threw.
- `NAVIGATION_FAILED` — navigate didn't reach a loaded state in time.
- `INTERNAL` — something on our side broke.

## Worked example: log in to a site

```javascript
// pseudocode for an external agent
const snap = await tools.snapshot({ url_or_ref: "https://example.com/login" });
const email_ref = findRefByAriaLabel(snap, "Email");
const password_ref = findRefByAriaLabel(snap, "Password");
const submit_ref = findRefByAriaLabel(snap, "Submit");

await tools.type_text({ ref: email_ref, text: "[email protected]" });
await tools.type_text({ ref: password_ref, text: process.env.PASSWORD });

// Sensitive-arg redaction will replace "password" with [REDACTED] in audit
// trails, but the actual call goes through (policy risk mode = HighAutonomy).

const sub = await tools.submit_form({ ref: submit_ref });
if (!sub.ok) throw new Error(`Login failed: ${sub.error?.message}`);

await tools.wait_for({ selector: ".dashboard", timeout_ms: 5000 });
const after = await tools.snapshot({ url_or_ref: "@self" });
```

## See also

- `SKILL.md` — agent-loadable version of this spec.
- `docs/RUNBOOK-DEV.md` — how to run NeuroBrowser locally.
- `docs/TESTING-NOTES.md` — which parts are automated vs manual.
- `src-tauri/src/runtime.rs` — implementation of the JS-RPC bridge that
  exposes these tools.
- `src/agent/policy.rs` — implementation of the policy gates.