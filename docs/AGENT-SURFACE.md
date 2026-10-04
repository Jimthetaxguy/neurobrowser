# NeuroBrowser — Agent Surface (spec-of-record)

Canonical agent-facing surface for the **shipped crate**. Update `SKILL.md`
with this file.

- **24 tools** on the agent surface. `default_tool_registry()` in
  `src/browser/mod.rs` registers the 22 browser tools.
  `default_tool_registry_with_memory()` adds `search_personal_memory` and
  `inspect_active_page`. `ReActAgent::with_memory` uses that 24-tool registry.
  `ReActAgent::new` keeps the 22 browser tools.
- Legacy tools accept CSS selectors (or pixels / a key). New scoped target tools
  consume `PageObservation.document` and a runtime-issued `target_id`. `PageSnapshot`
  keeps its compatibility shape and has no `ref_map`.
- Autonomy: `ReadOnly` / `Assisted` / `HighAutonomy` via `ActionPolicy`.
- Headless JSON-RPC is `ping` / `policy.*` / `snapshot`. `snapshot` returns a
  hardcoded stub (`url`, `title`, `viewport`, `tree: ""`), not a crate
  `PageSnapshot` and not a live WKWebView session.

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

## Shared observations and scoped actions

`observe_page` returns a version-1 `PageObservation` with exact source URL,
capabilities, document stamp, bounded text/links/tables, targets and omissions.
The serialized envelope is at most 64 KiB; raw HTML and field values are excluded.
HTTP has no scoped targets. Desktop targets contain IDs, roles, labels, state and
reviewed destinations, with no field values. Document IDs change on navigation and
revision changes invalidate relevant page state, including silent form-value edits.

`click_target`, `type_target`, `submit_target` and `scroll_target` take string arguments
`document` (JSON stamp) and `target_id`; typing also takes `text`, redacted in events.
Optional `postcondition` is JSON `{ "type": "url_equals", "url": "..." }` or
`{ "type": "text_contains", "text": "..." }`. Unknown arguments are rejected.

A scoped tool's `ToolResult.result` is a JSON `ActionReceipt` with dispatch
`not_dispatched`/`acknowledged`/`unknown`, page readiness and verification
`not_requested`/`satisfied`/`unsatisfied`/`unavailable`. Verification proves the
specified observed document condition, not remote transaction success. The run stops
on uncertain dispatch or unresolved readiness/verification; inspect before continuing.

External Rust clients can call `propose_tool_with_policy` without an AI provider call.
Approved execution must use `execute_approved_tool_with_policy` with latest host policy.
The old compatibility method uses only the stored proposal's policy. Grants bind exact
run/call/policy and reviewed page/target; IDs cannot manufacture authority and are consumed
once. Desktop policy changes acknowledge after an exclusive write lease installs them.

The desktop exposes `get_page_observation` and `execute_browser_tool` only to its
trusted control webview. Remote page webviews have report-only IPC, bound to native
caller and request ownership. No new MCP transport is shipped; the daemon below
remains its separate policy stub. Main-world page execution is not hostile-page isolation.

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

## Tools (24)

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

On `BrowserEngine`, click / type / submit / scroll fail with an
honest static-engine error (no live DOM). `keypress` / `back` / `forward` /
`reload` / `screenshot` use the `BrowserInterface` defaults (error) unless a
runtime overrides them. The Tauri runtime does not override `screenshot`.

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
Action: `Type`. Text and credential arguments are `[REDACTED]` in policy and
agent event payloads. Browser evidence excludes input values.

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
| `policy.set` | Replace the stored policy by deserializing `params` as an `ActionPolicy` object |
| `policy.evaluate` | Reads `params.tool` and `params.arguments`, then gates that call (no execution). Unknown names, including memory tools absent from the 17-tool risk catalog, fall back to `Destructive`. Headless evaluate builds an empty `PageSnapshot` (no URL, HTML, or text) before calling `ActionPolicy::evaluate`, so prompt-injection cannot fire, `is_cross_domain` is false with no host, and non-navigate tools never get a domain; allow/deny lists still apply to `navigate` via the argument URL |
| `snapshot` | Ignores `params`. Hardcoded stub `{ "url": "about:blank", "title": "", "viewport": { "width": 0, "height": 0, "scroll_x": 0, "scroll_y": 0 }, "tree": "" }` — not a crate `PageSnapshot` |

Unknown methods return `UNKNOWN_METHOD`. This is not a WKWebView session.

## Autonomy

`ActionPolicy::evaluate` gates by `ToolRisk.action` and domain lists.

| Level | Auto-allow | Otherwise |
|---|---|---|
| `ReadOnly` | `Read`, `Wait`, `Scroll` | `Block`, including `Navigate` |
| `Assisted` | `Read`, `Wait`, `Scroll`, `Navigate` that is not cross-domain; a hostless current page (`about:blank`, empty URL) is not cross-domain | `RequireApproval` |
| `HighAutonomy` | remaining non-high-impact actions | submit / purchase / auth / upload / message / destructive require approval |

This table applies only after the common gates. Denied tools/domains, off-list
domains, unsafe navigation schemes, and detected prompt injection block first.
Sensitive argument keys, sensitive tool metadata (including `type`), and explicit
approval-list matches return `RequireApproval` before mode evaluation in
`HighAutonomy`; ReadOnly action prohibitions block before approval triggers.

Credential keys are normalized across case, camelCase, and separators; matching
credential tokens (including `authorization`, `authentication`, `apiKey`,
`accessToken`, and `cardNumber`) are redacted to `[REDACTED]` in the decision.
The plain `text` argument is also redacted in decisions and events. Tool metadata still determines whether approval is needed.

## Policy gates

`ActionPolicy::evaluate` returns on the first matching gate:

1. `denied_tools` → `Block`.
2. Prompt-injection on the page → `Block`.
3. Unsafe navigation schemes (`javascript:` / `data:` / `file:` / …) → `Block`.
4. If the URL has a parsed host, block it when it appears in `denied_domains` or misses a non-empty allowlist. A rule matches that host or its subdomains (`example.com` matches `a.example.com`). If the URL has no parsed host, skip this gate.
5. ReadOnly action prohibitions → `Block`.
6. Sensitive argument keys or sensitive tool metadata → `RequireApproval`.
7. `approval_required_tools` → `RequireApproval`.
8. Remaining mode table.

Tauri: `get_action_policy` / `set_action_policy`. Daemon: `policy.get` /
`policy.set`.

Approval IDs are single-use lookup keys into private reviewed grants. Grants expire
after five minutes. `ReActAgent::approval_context` exposes only the reviewed URL,
document and target for a human approval card. Input values stay redacted.

## Error shape

Crate tools return `ToolResult { success: false, result: "<message>" }`.
The next system prompt lists each failure as `- <tool>: Error: <message>`.

`click`, `submit_form`, `keypress`, and `reload` distinguish execution from subsequent
page readiness. After an executed action, a loading timeout keeps
`success: true` and includes the readiness error in `result`, with guidance
to wait or inspect the page before continuing. It does not imply the action
failed or should be repeated. A submission dispatch is not confirmation
that the server accepted it.
If the action's runtime acknowledgment itself times out or its response
channel closes, the tool keeps `success: false` and reports an unknown
outcome: the action may have executed and must not be retried automatically.

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
This method only evaluates a proposed call; it does not execute it. `params.arguments` must be an object with string values; omission is an empty map, while other shapes return `VALIDATION`.

`policy.set` deserializes `params` as an `ActionPolicy`. `autonomy_level` is
snake_case: `read_only` | `assisted` | `high_autonomy`. `ActionPolicy` fields
have no serde defaults, so `allowed_domains`, `denied_domains`, `denied_tools`,
`approval_required_tools`, and `block_prompt_injection` must be present:

```json
{"id":"5","method":"policy.set","params":{"autonomy_level":"read_only","allowed_domains":["example.com"],"denied_domains":[],"denied_tools":[],"approval_required_tools":[],"block_prompt_injection":true}}
```

## See also

- `SKILL.md` — agent-loadable version.
- `docs/RUNBOOK-DEV.md` — build + run.
- `src/browser/mod.rs` — registry + `BrowserEngine`.
- `src/tools/memory_tools.rs` — `search_personal_memory` and `inspect_active_page`.
- `src/tools/mod.rs` — `BrowserInterface` / `PageSnapshot`.
- `src/agent/policy.rs` — policy gates.
