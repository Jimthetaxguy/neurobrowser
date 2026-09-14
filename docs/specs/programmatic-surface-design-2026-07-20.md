# NeuroBrowser — Programmatic Surface

## Current boundaries

1. **Rust library** owns `SessionManager`, `ReActAgent`, `ActionPolicy`,
   `BrowserInterface`, and the 17-tool default registry. `BrowserEngine` is an
   HTTP scraper; `TauriBrowserRuntime` drives desktop webviews.
2. **Headless daemon** is a policy protocol stub. Its newline-delimited JSON-RPC
   methods are `ping`, `policy.get`, `policy.set`, `policy.evaluate`, and a
   hardcoded `snapshot`. It does not navigate, execute tools, or run an agent.
3. **Future CLI and MCP clients** require real daemon browser wiring first.
   No CLI crate or CLI browser command is shipped.
4. **Direct embedding** links the library and supplies a real provider and browser.

## Proposed browser commands

A future CLI must map these 17 verbs to the current registry using CSS selectors.
These are a proposed contract, not callable daemon methods today. The stub
`snapshot` RPC and library `BrowserInterface::snapshot()` are outside the registry.

| Proposed verb | Registry tool | Arguments |
|---|---|---|
| navigate | navigate | url |
| wait | wait | none |
| query-dom | query_dom | selector |
| get-text | get_text | selector |
| get-links | get_links | none |
| get-prices | get_prices | none |
| get-tables | get_tables | none |
| click | click | selector |
| type | type | selector, text |
| scroll-to | scroll_to | selector |
| scroll-by | scroll_by | x, y |
| submit-form | submit_form | selector |
| keypress | keypress | key |
| screenshot | screenshot | none; runtime support required |
| back | back | none |
| forward | forward | none |
| reload | reload | none |

There are no shipped named `evaluate`, `get_attribute`, `wait_for`, or
`extract_text` tools. Screenshot is registered but neither current runtime
implements it. Unsupported browser capabilities must return errors.

## Shared contract and dependencies

Core types include `PageSnapshot`, `ActionPolicy`, `PolicyDecision`,
`AgentRunResult`, `AgentRunEvent`, `ToolDefinition`, and `ToolRisk`.
The current wire shape is `{id, method, params}` →
`{id, ok, result|error:{code,message}}`. Policy evaluation returns a successful
response containing an `Allow`, `RequireApproval`, or `Block` decision.
See [the agent surface](../AGENT-SURFACE.md) for fields and actual methods.

Real browser/session wiring and socket authorization must precede browser CLI
or MCP commands. Any future execution and approval flow must use the same policy
gates and browser capabilities as the desktop. Element-ref maps, a unified
facade, cross-process workers, and streaming RPC remain separate future work;
they are not prerequisites for documenting the current selector tools accurately.
