# NeuroBrowser — Agent Surface (spec-of-record)

Canonical agent-facing surface for the **shipped crate**. Update `SKILL.md`
with this file.

- **19 tools** on the agent surface. `default_tool_registry()` in
  `src/browser/mod.rs` registers the 17 browser tools.
  `default_tool_registry_with_memory()` adds `search_personal_memory` and
  `inspect_active_page`. `ReActAgent::with_memory` uses that 19-tool registry.
  `ReActAgent::new` keeps the 17 browser tools.
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

## Personal memory vs agent run memory

Two stores share the word "memory". They are not interchangeable.

| Store | Type | What it holds |
|---|---|---|
| Persistent personal memory | `neuro_memory::MemoryService` | Captured pages on disk. `search_personal_memory` and `inspect_active_page` close over `Arc<MemoryService>`. |
| In-run agent log | none in the shipped crate | `ReActAgent` does not keep a separate episodic store. The memory tools do not write the page index from the run log, and they do not read one. |

`inspect_active_page` reads the current URL from the browser snapshot. It
returns that URL's captured blocks, or a `capture denied: ...` error when
`CapturePolicy` refuses the URL. `search_personal_memory` ignores the browser
argument and searches the index.

## Tools (19)

Arguments are `HashMap<String, String>`. Results are `ToolResult`
(`tool_name`, `result`, `success`).

`ToolDefinition` exposes `name`, `description`, `arguments`, and `risk`.
Every `BrowserTool` must implement `definition()`; there is no default risk.
The registry keys each tool by `definition().name`.
`ToolRisk` contains the action category plus the `sensitive` and
`externally_visible` flags; both flags default to `false`. There is no
risk-level or tool-version field. The catalog below names each action and
identifies the two tools that set a flag.

The system prompt's tool list is rendered from these definitions: name,
description, and each argument with a `(required)` flag. The two memory tools
are listed only when `AiContext::personal_memory` is set.

On `BrowserEngine`, click / type / submit / scroll / keypress fail with an
honest static-engine error (no live DOM). `back` / `forward` / `reload` /
`screenshot` use the `BrowserInterface` defaults (error) unless a runtime
overrides them. The Tauri runtime does not override `screenshot`.

### 1. `navigate` — `url`

Navigate / fetch. Action: `Navigate`.

### 2. `wait`

Wait for navigation to settle. No args. Action: `Wait`.

### 3. `query_dom` — `selector`

Query by CSS selector. Returns a text dump of matches, or
`No elements found`. Invalid CSS returns an error. Action: `Read`.

### 4. `get_text` — `selector`

Concatenated text of matches. Action: `Read`.

### 5. `get_links`

All links (`text - href`). Action: `Read`.

### 6. `get_prices`

Price-like strings from the snapshot. Action: `Read`.

### 7. `get_tables`

`Table N: H headers, R rows` per table. Action: `Read`.

### 8. `click` — `selector`

Click. Action: `Click`.

### 9. `type` — `selector`, `text`

Type into an input. Tool metadata sets `sensitive: true`, so policy requires approval.
Action: `Type`. Redaction is based on argument keys; do not assume the
plain `text` key is redacted from every tool or event payload.

### 10. `scroll_to` — `selector`

Scroll an element into view. Action: `Scroll`.

### 11. `scroll_by` — `x`, `y`

Pixel deltas. Action: `Scroll`.

### 12. `submit_form` — `selector`

Submit a form (or an element inside one). Action: `Submit`,
with `externally_visible: true`.

### 13. `keypress` — `key`

Send a key (`Enter`, `Escape`, …). Action: `Keypress`.

### 14. `screenshot`

Registered. `BrowserInterface::screenshot` defaults to
`Err("screenshot is not supported by this browser")`. No PNG path is
wired. Action: `Screenshot`.

### 15. `back`

History back. Default: not supported. Action: `Back`.

### 16. `forward`

History forward. Default: not supported. Action: `Forward`.

### 17. `reload`

Reload. Default: not supported. Action: `Reload`.

### 18. `search_personal_memory` — `query`, optional `limit`

Search `MemoryService` (persistent personal memory, not an in-run agent log).
Ignores the browser argument. `limit` defaults to 5 and caps at 20. A miss
is `No personal memory matches.` Action: `Read`. Registered by
`default_tool_registry_with_memory` / `ReActAgent::with_memory`.

### 19. `inspect_active_page`

Captured blocks for the browser's current URL, or `capture denied: ...` when
`CapturePolicy` refuses that URL. A URL with no committed blocks is
`No captured content for <url>`. An argument named `url` is not consulted.
Action: `Read`. Same registration as `search_personal_memory`.

## Headless JSON-RPC

`src-tauri/src/headless_bin/main.rs` (`--features headless`). Newline-delimited
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

`ActionPolicy::evaluate` returns on the first matching gate:

1. `denied_tools` → `Block`.
2. Prompt-injection on the page → `Block`.
3. Unsafe navigation schemes (`javascript:` / `data:` / `file:` / …) → `Block`.
4. `denied_domains` / non-empty `allowed_domains` → `Block`.
5. Sensitive argument keys or sensitive tool metadata → `RequireApproval`.
6. `approval_required_tools` → `RequireApproval`.
7. Mode table.

Tauri: `get_action_policy` / `set_action_policy`. Daemon: `policy.get` /
`policy.set`.

## Error shape

Crate tools return `ToolResult { success: false, result: "<message>" }`.
The next system prompt lists each failure as `- <tool>: Error: <message>`.

A model call to a registered tool that omits a required argument (or sets it
to `""`) is not dropped and does not run. `ReActAgent` emits
`ToolCallResult { success: false }` with
`Error: missing required argument(s): <names>` and the model sees it on the
next turn. Whitespace counts as a value; the tool decides whether it is valid.

Daemon errors:

```json
{ "ok": false, "error": { "code": "INTERNAL", "message": "..." } }
```

Codes the daemon emits include `BAD_REQUEST`, `INTERNAL`, `VALIDATION`, and `UNKNOWN_METHOD`.
A successful `policy.evaluate` request returns `ok: true` with its policy decision
in `result`; approval and blocking are decision outcomes, not transport errors:

```json
{"id":"2","ok":true,"result":{"outcome":"RequireApproval","reasons":["Tool call contains sensitive input"],"risk_flags":["SensitiveArgument"],"redacted_arguments":{"apiKey":"[REDACTED]"}}}
```

The daemon outcomes are `Allow`, `RequireApproval`, and `Block` (the Rust
serde representation uses snake_case, but this dispatcher formats enum names).
This method only evaluates a proposed call; it does not execute it.

## See also

- `SKILL.md` — agent-loadable version.
- `docs/RUNBOOK-DEV.md` — build + run.
- `src/browser/mod.rs` — registry + `BrowserEngine`.
- `src/tools/memory_tools.rs` — `search_personal_memory` and `inspect_active_page`.
- `src/tools/mod.rs` — `BrowserInterface` / `PageSnapshot`.
- `src/agent/policy.rs` — policy gates.
