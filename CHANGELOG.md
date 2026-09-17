# Changelog

All notable changes to NeuroBrowser are recorded here. Dates are UTC.
The 0.1.1 heading is a documentation/release label; both Rust package versions
remain 0.1.0 until an actual version bump and release tag.

## Unreleased — maintenance through 2026-09-14

### Fixed

- Policy canonicalizes supported tool aliases before deny and approval checks.
  Navigation domains are checked case-insensitively; unsafe URL schemes are
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
  navigate or run history commands on another tab.
- Frontend dependency lockfile updated within the existing package constraints;
  the moderate-severity npm audit gate is retained in the spec and story.

### Removed

- Unused agent scaffolding, worker spawn/inbox plumbing, and unused exports/tests.
  Worker summary types and empty compatibility readers remain; worker execution
  is not shipped.
- The unused Tauri shell plugin and four unused IPC commands (`get_page_info`,
  `list_sessions`, `list_workers`, `get_worker`). The command manifest and
  generated permissions now cover the 20 actual commands.
- The redundant headless `policy.snapshot` method; `policy.get` reads policy.
- Obsolete status dumps and misleading architectural claims. The root glossary,
  project summary, agent surface, and skill describe the maintained boundaries.

### Verification

`verify.sh` runs Rust formatting, clippy with warnings denied, all library tests,
frontend installation/build, locked desktop and headless cargo checks, explicit
headless binary tests, and the library release build. It respects the caller's
`CARGO_TARGET_DIR`. CI's guard checks fail on prohibited patterns or merge markers.
The development runbook also documents native resource generation and Xcode build.

## [0.1.1] — 2026-07-08

The first documented release after the live Tauri browser runtime was integrated.
Later removals and fixes are recorded above; the current surface is described in
[the agent contract](docs/AGENT-SURFACE.md) and [the skill](SKILL.md).

### Added

- A Tauri desktop shell with a React tab strip, URL bar, chat panel, command
  palette, settings, policy controls, and agent run events. Rust owns a real
  child webview per page on macOS.
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
- Native function calling in the ReAct loop, full screenshot support, and
  budget-capped real-provider integration coverage. Unit test provider fixtures
  do not constitute real-provider integration verification.

## [0.1.0] — 2026-02-23

The foundation library included providers, a ReAct agent, an HTTP scraper browser,
sessions, and DOM tools. The desktop shell was still incomplete.
