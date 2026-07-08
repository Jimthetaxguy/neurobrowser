---
id: SPEC-AUTONOMOUS-BROWSER-AGENT
status: implemented
owner: project
updated: 2026-05-21
---

# Autonomous Browser Agent Core

## Goal

Build a provider-agnostic browser agent core that can inspect and act on real web
pages while keeping autonomy visible, policy-gated, and auditable. Rust remains the
authority for sessions, pages, providers, browser actions, policy decisions, and
run events.

## Runtime Contract

The run-oriented API is the stable control surface:

| Command | Purpose |
| --- | --- |
| `start_agent_run(sessionId, pageId, prompt)` | Starts a policy-evaluated agent run. |
| `submit_approval(runId, approved, message)` | Resolves an approval-gated tool call. |
| `cancel_agent_run(runId)` | Cancels a pending run. |
| `get_action_policy()` | Reads the active autonomy and risk policy. |
| `set_action_policy(policy)` | Updates the active autonomy and risk policy. |

`ask(sessionId, pageId, prompt)` remains a compatibility command. New autonomous
work should use the run-oriented API so approvals, blocks, and results are visible
as structured events.

## Tool Contract

Browser tools must expose:

- a stable tool name,
- JSON argument schema metadata,
- risk metadata,
- a deterministic argument parser,
- a structured result or structured rejection.

The shared tool inventory covers navigation, snapshot, query, click, type,
keypress, scroll, submit, screenshot, back, forward, reload, and close-tab actions.

## Policy Requirements

Default autonomy mode is `assisted`. The policy layer must classify each proposed
tool call before execution using action type, target domain, sensitive arguments,
page text, and configured allow/deny rules.

Policy behavior:

- reads, snapshots, scrolling, and allowed same-domain navigation may run without
  approval,
- sensitive typing, form submission, authenticated flows, downloads/uploads,
  purchases, messages, destructive actions, and denylisted domains require approval
  or blocking,
- prompt-injection or suspicious-page patterns block execution,
- deny rules take precedence over allow rules,
- provider-specific behavior must not exist in the policy layer.

## Audit Trail

Each proposed, blocked, approval-requested, approved, rejected, executed, cancelled,
and completed action must record:

- run id,
- page id,
- tool name,
- redacted arguments,
- policy decision,
- risk flags,
- timestamp,
- structured result or rejection.

## Frontend Requirements

The React control surface must show:

- run status,
- current policy mode,
- pending approval cards,
- risk reason,
- action history.

Approval-required actions must not execute silently.

## Verification

Required checks before promoting changes against this spec:

```bash
./verify.sh
npm run build
npm audit --audit-level=moderate
npm run smoke:desktop
```

AppKit parity remains deferred unless the Tauri child-webview path fails a real
browser smoke test.
