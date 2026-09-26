# Changelog

All notable changes to NeuroBrowser are recorded here. Dates are UTC.
The 0.1.1 heading is a documentation/release label; both Rust package versions
remain 0.1.0 until an actual version bump and release tag.

## Unreleased — maintenance through 2026-09-14

### Changed

- The system prompt's tool list is generated from each tool's
  `ToolDefinition` (memory tools only when attached) instead of a
  hand-maintained list.
- Anthropic user turns carry only the prompt. Tool results reach the model
  once, through the system prompt.
- Ollama builds its endpoint with the shared `base_url` helper, like OpenAI and
  Anthropic. A trailing slash is trimmed and a blank value falls back to the
  default.
- Tool calls that omit a required argument (absent or `""`) no longer run.
  Before this change they ran with empty defaults or reached the approval gate.
  The agent now records `Error: missing required argument(s): …` and returns it
  to the model. That turn does not complete the run. A `ToolCall` with no
  `arguments` object is read as an empty one and checked the same way. So is
  a legacy `Action:` call to a known tool with empty parentheses: `navigate()`
  is reported, and `wait()` now runs instead of being dropped. A legacy line
  must end with `)`. A truncated `back(` or `navigate(https://ex` is dropped;
  before, it ran.
- The prompt shows each failed tool result with its message, not a bare
  `Error`.
- `BrowserTool::definition()` is required. Before, a tool that did not
  override it defaulted to `Read` risk, which `Assisted` allows without
  approval. The registry keys each tool by `definition().name`.

### Fixed

- Removed the unused `futures` dependency from both library and desktop
  lockfiles so locked Tauri builds remain reproducible.
- Navigation domains are checked case-insensitively; unsafe URL schemes are
  rejected without DNS lookups. ReadOnly blocks navigation.
- Credential keys are recognized across case, camelCase, and separators, require
  approval, and are redacted in policy decisions. Benign words such as `author`
  and `discard` remain unchanged.
- Headless argument parsing rejects unknown options and missing socket paths;
  `--socket=VALUE` can represent a path beginning with a dash.
- Empty-history Back/Forward no longer leave the desktop loading indicator stuck.
- The native AppKit app bundles its generated React controls, and its address
  field describes URL/domain entry after removal of native agent/search controls.
- AppKit maps React `pageId` to a WKWebView, boots a single first tab from
  React `createPage`, and leaves Cmd+T/W to the native File menu. Native menu
  IDs cannot collide with React IDs; tab changes synchronize to both controls.
  Snapshots remain associated with their source page, and stale page IDs cannot
  navigate or run history commands on another tab. Background tab updates preserve
  URL edits, and native selection/snapshot bursts use the new active page.
- Frontend dependency lockfile updated within the existing package constraints.

### Removed

- `BrowserTool::name()` / `description()` (the strings live in
  `definition()`) and `ToolRegistry`'s `Default` impl, which built an empty
  registry. Use `ToolRegistry::new()` and `register`, or
  `default_tool_registry()`.
- Unread tool-result echo fields: `ToolResult.arguments` (kept raw `type`
  values after the result string was de-leaked), unused
  `ToolArgumentDefinition.sensitive`, and constant snapshot `selector` fields
  on links/forms/tables/prices.
- Unused agent scaffolding, worker spawn/inbox plumbing, Worker summary types,
  empty compatibility readers, and unused exports/tests. Worker execution was
  never shipped.
- The unused Tauri shell plugin and unused IPC commands (`get_page_info`,
  `list_sessions`, `list_workers`, `get_worker`, `ask`). The command manifest and
  generated permissions now cover the 18 actual commands.
- The redundant headless `policy.snapshot` method; `policy.get` reads policy.
- Obsolete status dumps and misleading architectural claims. README, the agent
  surface, ADR-001, and the skill describe the maintained boundaries.
- Redundant status glossary, project summary, and the implemented GitSpec pair.

## [0.1.1] — 2026-07-08

The first documented release after the live Tauri browser runtime was integrated.
Later removals and fixes are recorded above; the current surface is described in
[the agent contract](docs/AGENT-SURFACE.md) and [the skill](SKILL.md).

### Added

- A Tauri desktop shell with a React tab strip, URL bar, chat panel,
  provider/policy selects, approval card, action history, and agent run
  events. Rust owns a real child webview per page on macOS.
- A default library registry of 17 CSS-selector tools: `navigate`, `wait`,
  `query_dom`, `get_text`, `get_links`, `get_prices`, `get_tables`, `click`,
  `type`, `scroll_to`, `scroll_by`, `submit_form`, `keypress`, `screenshot`,
  `back`, `forward`, and `reload`. `snapshot()` is a separate browser interface
  method. Screenshot is registered but has no implementation in either runtime.
- Agent run APIs with structured approval/block results and the `ActionPolicy`
  modes ReadOnly, Assisted, and HighAutonomy. Decisions are `Allow`,
  `RequireApproval`, or `Block`; common safety gates apply before mode rules.
- Episodic LLM/tool-call records, request/tool/error metrics, and bounded
  conversation history for the ReAct agent.
- A feature-gated headless policy protocol over a Unix socket with loopback TCP
  fallback after bind failure. Its current methods are `ping`, `policy.get`,
  `policy.set`, `policy.evaluate`, and a hardcoded `snapshot`. It does not run a
  browser or execute tools.
- A development verifier, agent documentation, and policy/agent regression tests.

### Still unimplemented

- Real browser and agent execution through the headless protocol.
- CLI and MCP clients, cross-process worker execution, and a worker sidebar.
- Native function calling in the ReAct loop, full screenshot support, visual
  regression tests, and budget-capped real-provider integration coverage. Unit
  test provider fixtures do not constitute real-provider integration verification.

## [0.1.0] — 2026-02-23

The foundation library included providers, a ReAct agent, an HTTP scraper browser,
sessions, and DOM tools. The desktop shell was still incomplete.
