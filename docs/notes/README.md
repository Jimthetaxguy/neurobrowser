# Notes Policy

NeuroBrowser uses a lightweight GitSpec-style split between committed project
knowledge and local working notes.

Committed docs:

- `docs/specs/` contains durable product and architecture specs.
- `docs/stories/` contains user-facing slices with acceptance criteria.
- `docs/adr/` contains accepted architecture decisions.
- `docs/notes/` contains public note policy and templates only.

Ignored local docs:

- `docs/notes/local/` is for local worklogs, raw prompts, investigation traces,
  branch-switch scratch, and personal process notes.
- `docs/notes/agents/local/` is for local agent scratch notes.
- `docs/private/` is for local-only material that must never be promoted directly.
- `.ledger/local/` is for local-only change tracking.

The canonical local worklog path is:

```text
docs/notes/local/neurobrowser-worklog.md
```

Promote only distilled, project-relevant decisions into committed specs, stories,
or ADRs. Do not commit credentials, absolute local paths, raw prompt transcripts,
private personal context, provider secrets, or machine-specific artifacts.
