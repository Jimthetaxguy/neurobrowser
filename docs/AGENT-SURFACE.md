# NeuroBrowser — Agent Surface (spec-of-record)

Canonical agent-facing surface for the **shipped crate**. Update `SKILL.md`
with this file.

- **17 tools** from `default_tool_registry()` in `src/browser/mod.rs`.
- CSS selectors (or pixels / a key). There is no `ref_map`
  (`PageSnapshot` has no such field).
- Autonomy: `ReadOnly` / `Assisted` / `HighAutonomy` via `ActionPolicy`.
- Headless JSON-RPC is `ping` / `policy.*` / `snapshot` (hardcoded stub), not
  a live WKWebView session.

Desktop is macOS WKWebView. The separate library `BrowserEngine` uses
reqwest+scraper. The headless daemon does not construct either browser.

Not shipped as named tools: `evaluate`, `get_attribute`, `wait_for`,
`extract_text`.

## PageSnapshot

`BrowserInterface::snapshot()` (library method, **not** a registry tool)
returns:

```text
url, title, html, text,
viewport_width, viewport_height, scroll_x, scroll_y,
interactive_ready, links, images, forms, prices, tables
```

No element-ref map. No ARIA tree field.

## Tools (17)

Arguments are `HashMap<String, String>`. Results are `ToolResult`
(`tool_name`, `arguments`, `result`, `success`).

On `BrowserEngine`, click / type / submit / scroll / keypress fail with an
honest static-engine error (no live DOM). `back` / `forward` / `reload` /
`screenshot` use the `BrowserInterface` defaults (error) unless a runtime
overrides them. The Tauri runtime does not override `screenshot`.

### 1. `navigate` — `url`

Navigate / fetch. Risk: `Navigate`, medium.

### 2. `wait`

Wait for navigation to settle. No args. Risk: `Wait`, low.

### 3. `query_dom` — `selector`

Query by CSS selector. Returns a text dump of matches, or
`No elements found`. Risk: `Read`, low.

### 4. `get_text` — `selector`

Concatenated text of matches. Risk: `Read`, low.

### 5. `get_links`

All links (`text - href`). Risk: `Read`, low.

### 6. `get_prices`

Price-like strings from the snapshot. Risk: `Read`, low.

### 7. `get_tables`

`Table N: H headers, R rows` per table. Risk: `Read`, low.

### 8. `click` — `selector`

Click. Risk: `Click`, medium.

### 9. `type` — `selector`, `text`

Type into an input. Tool metadata is sensitive, so policy requires approval.
Risk: `Type`, high. Redaction is based on argument keys; do not assume the
plain `text` key is redacted from every tool or event payload.

### 10. `scroll_to` — `selector`

Scroll an element into view. Risk: `Scroll`, low.

### 11. `scroll_by` — `x`, `y`

Pixel deltas. Risk: `Scroll`, low.

### 12. `submit_form` — `selector`

Submit a form (or an element inside one). Risk: `Submit`, high,
externally visible.

### 13. `keypress` — `key`

Send a key (`Enter`, `Escape`, …). Risk: `Keypress`, medium.

### 14. `screenshot`

Registered. `BrowserInterface::screenshot` defaults to
`Err("screenshot is not supported by this browser")`. No PNG path is
wired. Risk: `Screenshot`, low.

### 15. `back`

History back. Default: not supported. Risk: `Back`, low.

### 16. `forward`

History forward. Default: not supported. Risk: `Forward`, low.

### 17. `reload`

Reload. Default: not supported. Risk: `Reload`, low.

## Headless JSON-RPC

`src-tauri/src/bin/headless.rs` (`--features headless`). Newline-delimited
`{id, method, params}` → `{id, ok, result|error}`.

| Method | What it does |
|---|---|
| `ping` | `{ "pong": true }` |
| `policy.get` | Current `ActionPolicy` |
| `policy.set` | Replace `ActionPolicy` |
| `policy.evaluate` | Gate a tool name + args (no execution) |
| `snapshot` | Hardcoded `about:blank` stub — not a crate `PageSnapshot` |

Unknown methods return `UNKNOWN_METHOD`. This is not a WKWebView session.

## Autonomy

`ActionPolicy::evaluate` gates by `ToolRisk.action` and domain lists.

| Level | Auto-allow | Otherwise |
|---|---|---|
| `ReadOnly` | `Read`, `Wait`, `Scroll` | `Block`, including `Navigate` |
| `Assisted` | `Read`, `Wait`, `Scroll`, same-domain `Navigate` | `RequireApproval` |
| `HighAutonomy` | remaining non-high-impact actions | submit / purchase / auth / upload / message / destructive require approval |

This table applies only after the common gates. Denied tools/domains, off-list
domains, unsafe navigation schemes, and detected prompt injection block first.
Sensitive argument keys, sensitive tool metadata (including `type`), and explicit
approval-list matches return `RequireApproval` before mode evaluation, even in
`HighAutonomy` or `ReadOnly`.

Credential keys are normalized across case, camelCase, and separators; matching
credential tokens (including `authorization`, `authentication`, `apiKey`,
`accessToken`, and `cardNumber`) are redacted to `[REDACTED]` in the decision.
Sensitive tool metadata does not redact unrelated keys such as plain `text`.

## Policy gates

1. `denied_tools` / `denied_domains` → `Block`.
2. Non-empty `allowed_domains` → off-list `Block`.
3. `javascript:` / `data:` / `file:` / … navigate → `Block`.
4. Prompt-injection on the page → `Block`.
5. `approval_required_tools` → `RequireApproval`.

Tauri: `get_action_policy` / `set_action_policy`. Daemon: `policy.get` /
`policy.set`.

## Error shape

Crate tools return `ToolResult { success: false, result: "<message>" }`.
Daemon errors:

```json
{ "ok": false, "error": { "code": "INTERNAL", "message": "..." } }
```

Codes the daemon emits include `INTERNAL`, `VALIDATION`, and `UNKNOWN_METHOD`.
A successful `policy.evaluate` request returns `ok: true` with its policy decision
in `result`; approval and blocking are decision outcomes, not transport errors:

```json
{"id":"2","ok":true,"result":{"outcome":"RequireApproval","reasons":["Tool call contains sensitive input"],"risk_flags":["SensitiveArgument"],"redacted_arguments":{"apiKey":"[REDACTED]"}}}
```

The daemon outcomes are `Allow`, `RequireApproval`, and `Block` (the Rust
serde representation uses snake_case, but this dispatcher formats enum names).
This method
only evaluates a proposed call; it does not execute it.

## See also

- `SKILL.md` — agent-loadable version.
- `docs/RUNBOOK-DEV.md` — build + run.
- `src/browser/mod.rs` — registry + `BrowserEngine`.
- `src/tools/mod.rs` — `BrowserInterface` / `PageSnapshot`.
- `src/agent/policy.rs` — policy gates.
