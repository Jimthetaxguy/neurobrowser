---
id: STORY-001
spec: SPEC-AUTONOMOUS-BROWSER-AGENT
status: implemented
priority: P0
updated: 2026-05-21
---

# Autonomous Browser Agent Core

## User Need

As a NeuroBrowser user, I need the agent to browse real pages, propose actions,
pause for risky steps, and leave an inspectable history so I can understand and
control autonomous web workflows.

## Acceptance Criteria

- Agent runs start through `start_agent_run` and return structured run events.
- Tool calls are parsed from provider output into provider-agnostic structured JSON.
- `ActionPolicy` classifies allowed, approval-required, and blocked actions.
- Sensitive arguments are redacted in approval and audit payloads.
- Prompt-injection patterns in page text stop execution.
- The React shell shows policy mode, pending approvals, run status, and action
  history.
- Compatibility `ask` behavior remains available.
- Verification passes through the root Rust checks, frontend build, desktop smoke,
  and audit gate.

## Implementation Links

- Spec: `docs/specs/SPEC-AUTONOMOUS-BROWSER-AGENT.md`
- Frontend architecture: `docs/frontend-architecture-spike.md`
- Policy layer: `src/agent/policy.rs`
- Run API: `src/agent/mod.rs`
- Tool metadata: `src/tools/contracts.rs`
- Tauri commands: `src-tauri/src/main.rs`
- React surface: `src-tauri/src/App.jsx`
- Tests: `tests/action_policy.rs`, `tests/autonomous_agent.rs`

## Verification

The implemented slice is expected to pass:

```bash
./verify.sh
npm run build
npm audit --audit-level=moderate
npm run smoke:desktop
```
