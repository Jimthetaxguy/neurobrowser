# NeuroBrowser — Agent Surface (spec-of-record)

Canonical agent-facing surface for the **shipped crate**. Update `SKILL.md`
with this file.

- **17 tools** from `default_tool_registry()` in `src/browser/mod.rs`.
- CSS selectors (or pixels / a key). There is no `ref_map`
  (`PageSnapshot` has no such field).
- Autonomy: `ReadOnly` / `Assisted` / `HighAutonomy` via `ActionPolicy`.
- Headless JSON-RPC is `ping` / `policy.*` / `snapshot` (scraper/stub), not
  a live WKWebView session.

Desktop is macOS WKWebView. Headless / `BrowserEngine` is reqwest+scraper.

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

Type into an input. `text` is sensitive (not echoed in the result).
Risk: `Type`, high.

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
| `policy.snapshot` | Current policy JSON |
| `snapshot` | Hardcoded `about:blank` stub — not a crate `PageSnapshot` |

Unknown methods return `UNKNOWN_METHOD`. This is not a WKWebView session.

## Autonomy

`ActionPolicy::evaluate` gates by `ToolRisk.action` and domain lists.

| Level | Auto-allow | Otherwise |
|---|---|---|
| `ReadOnly` | `Read`, `Wait`, `Scroll`, `Navigate` | `Block` |
| `Assisted` | `Read`, `Wait`, `Scroll`, same-domain `Navigate` | `RequireApproval` |
| `HighAutonomy` | allowed tools run | denied domains still `Block` |

Sensitive args (`type`, or keys matching
`password|token|secret|api_key|apikey|ssn|social|credit|card|cvv|otp|auth`)
→ `RequireApproval` and `[REDACTED]` in the audit trail.

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

Codes the daemon actually emits include `INTERNAL`, `VALIDATION`,
`UNKNOWN_METHOD`, plus policy outcomes from `policy.evaluate`.

## See also

- `SKILL.md` — agent-loadable version.
- `docs/RUNBOOK-DEV.md` — build + run.
- `src/browser/mod.rs` — registry + `BrowserEngine`.
- `src/tools/mod.rs` — `BrowserInterface` / `PageSnapshot`.
- `src/agent/policy.rs` — policy gates.
