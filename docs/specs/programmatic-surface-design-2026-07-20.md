# NeuroBrowser — Programmatic Surface Design (wired daemon + MCP + CLI + library)

> Generated 2026-07-20 from the `neurobrowser-programmatic-surface-design` workflow (5 source-grounded investigators -> synthesis).
> Goal: make NeuroBrowser callable programmatically across four composable surfaces James selected — **one core, thin clients, not four silos.**
> Companion to [../goals/goal-autoresearch-optimize-2026-07-20.md](../goals/goal-autoresearch-optimize-2026-07-20.md).

## Architecture

One core, thin clients — four layers, not four silos.

LAYER 1 (core library, `neurobrowser` crate, already exists): the single source of truth. It owns `SessionManager` (session/page/worker registry), `ReActAgent` (`execute_with_policy`/`execute_approved_tool`/`execute_stream`), `ActionPolicy` (autonomy + injection/redaction), the `BrowserInterface` trait with its two implementations (`BrowserEngine` non-rendering reqwest+scraper, and Tauri's `TauriBrowserRuntime`), the `ToolRegistry`/12-tool surface, and the serde-derived contract types. We ADD one embedding facade here — `NeuroBrowser<B: BrowserInterface = BrowserEngine>` — that bundles one browser + one agent + one policy (the unit nothing currently owns, since `PageHandle.runtime_id` is a bare label with no engine attached). The facade is the in-process Rust entry point AND the canonical vocabulary the other three surfaces mirror method-for-method (`ask`/`ask_with_policy`/`resume_approved` ↔ daemon `ask`/`run`/`approval.resolve`).

LAYER 2 (the ONE running process): the wired headless daemon (`src-tauri/src/bin/headless.rs`). It becomes a thin newline-delimited JSON-RPC front end over the SAME `SessionManager`/`ReActAgent`/`BrowserEngine` stack the Tauri desktop app drives — replacing today's hardcoded `about:blank` snapshot stub and the single process-global `SessionState` (shared `ActionPolicy` across every connection) with a `DaemonState` holding `Arc<SessionManager>`, a `HeadlessEngineRegistry` (page_id → `Arc<BrowserEngine>`), a shared immutable `ToolRegistry`, and per-connection/per-session isolation. This process is the one place browser+agent state lives; humans (Tauri) and programmatic clients attach to the same objects.

LAYER 3 (thin peer clients over the daemon socket): the MCP server (`neurobrowser-mcp`, new sibling repo) and the CLI (`neurobrowser`, new sibling crate) are BOTH stateless JSON-RPC clients that open the Unix socket (`NEUROBROWSER_SOCKET`), write one request line, read one response line. Neither re-embeds `BrowserEngine`/`ReActAgent` in-process — doing so would fork a second invisible browser session and defeat the human-in-the-loop premise. They depend on the core crate ONLY for the shared serde types (never the engine/agent themselves).

LAYER 1-direct (bypass): a Rust program that wants no daemon links the crate and uses the `NeuroBrowser` facade directly. This is why the facade's Rust method names and the daemon's RPC method names are deliberately kept in lockstep — the daemon is a transport over the facade's semantics, not a divergent second vocabulary.

Data flow for a programmatic browse: client (MCP/CLI) → JSON-RPC line → daemon dispatch() → SessionManager.get_page → BrowserEngine/agent call (wrapped in spawn_blocking because BrowserEngine uses reqwest::blocking) → contract struct serialized back → client renders. Streaming `run` forwards `StreamEvent`s as `{ok:true,stream:true,result:{event}}` lines terminated by a final `stream:false` result.

## Shared contract (every surface honors this)

Every surface honors ONE contract, in three parts, all already serde-derived in the core crate (zero-cost reuse):

1. CONTRACT TYPES (the wire + return vocabulary): `PageSnapshot` (extended with a new `#[serde(default)] ref_map: HashMap<String, RefMapEntry>` — today the JS `collectRefMap()` builds it but serde silently DROPS it because the struct has no field, even in Tauri); `ActionPolicy`/`AutonomyLevel`/`PolicyDecision`/`PolicyOutcome`/`RiskFlag`; `AgentRunResult`/`AgentRunEvent`/`AgentRunStatus`; `ToolDefinition`/`ToolRisk`/`ToolAction`/`RiskLevel` (extended per-definition with an `effective_under_engine: bool` hint, since BrowserEngine's click/type/submit are inert no-ops); `ProviderConfig`/`ProviderType`; `WorkerSpec`/`WorkerSummary`/`WorkerSnapshot`/`WorkerStatus`. The CLI and daemon deserialize daemon responses DIRECTLY into these (no hand-rolled DTOs); the MCP crate path-depends on the core crate for exactly these types and nothing else. Activate the already-declared-but-dead `schemars` dependency by deriving `JsonSchema` on this exact set so MCP tool `inputSchema`s generate from the same structs the daemon's `policy.evaluate` uses — no hand-maintained schema copy that drifts.

2. WIRE PROTOCOL (JSON-RPC over the Unix socket): newline-delimited `{id, method, params}` → `{id, ok, result|error:{code,message}}`, plus an additive optional `stream: bool` field for the streamed `run`. Canonical method names, agreed by all clients: `session.create/list/close`, `page.create/close/list`, `navigate`, `snapshot`, `tool.exec`, `list_tools` (+ `tools.list` returning `Vec<ToolDefinition>`), `ask`, `run` (streaming), `approval.resolve`, `run.cancel`, `worker.spawn/list/get`, `policy.get/set/evaluate/snapshot`, `skills.get`. Every method takes explicit `session_id`/`page_id` in params (mirrors main.rs Tauri command signatures 1:1). The 5 error codes and `@eN` ref model from `docs/AGENT-SURFACE.md` are the spec-of-record the per-tool CLI verbs and MCP tool schemas match verbatim.

3. POLICY / APPROVAL SEMANTICS: `ActionPolicy::evaluate` is the single gate; the two-call approval flow (`ask`→`AwaitingApproval`+`approval_id`→`approval.resolve`) is reused identically by the Tauri UI, the MCP `neurobrowser_approve` tool, and the CLI `approval submit` — no surface gets a laxer path than the desktop app. `PolicyDecision.reasons` and `RiskFlag`s pass through verbatim to every client. Redaction of sensitive args happens once, in the core, before audit.

## Per-surface decisions

### Library API (in-process Rust embedding)
- **Decision:** EXTEND the existing neurobrowser crate — no new crate. Add a NeuroBrowser<B: BrowserInterface = BrowserEngine> facade (owns browser+agent+policy, mirrors SessionManager::create_page's provider-wiring), curate the pub surface to one canonical import path per symbol (keep facade/BrowserEngine/PageConfig/AgentConfig/ReActAgent/ActionPolicy/AgentRun*/PageSnapshot/ProviderConfig at root; demote observability/memory/worker internals to submodule-only), fix Cargo.toml version drift vs CHANGELOG 0.1.1 + add license/repository/metadata, add #![warn(missing_docs)] + examples/. This IS the shared vocabulary and the foundation the other three mirror.
- **Depends on:** Nothing external — foundational. Its facade generic-over-BrowserInterface stays compatible with NB-18 EngineAdapter.

### Headless daemon (the one running process)
- **Decision:** WIRE the existing src-tauri/src/bin/headless.rs binary in place — do not rewrite. Replace the global SessionState with DaemonState (Arc<SessionManager> + HeadlessEngineRegistry page_id->Arc<BrowserEngine> + shared ToolRegistry + pending_approvals), move ActionPolicy off the process-global onto per-session state, replace the about:blank snapshot stub with a real BrowserEngine snapshot carrying ref_map, add all the new JSON-RPC methods, wrap BrowserEngine calls in spawn_blocking. Be honest that click/type/submit are inert under BrowserEngine (surface effective_under_engine).
- **Depends on:** Library Phase 0 (per-session policy + decision log on SessionManager, ref_map on PageSnapshot, click_ref/type_text_ref/submit_form_ref on BrowserInterface). Single narrow integration point for the in-flight peer-cred authz task (tag connection uid/pid, check owns(session_id) once before dispatch).

### MCP server
- **Decision:** NEW top-level crate neurobrowser-mcp in its OWN repo (sibling to browser-mcp-rs/macos-mcp-rs). Do NOT extend browser-mcp-rs — its own AGENTS.md states the actuator-boundary rule (actuator.browser = generic chromiumoxide/CDP, no ReActAgent/ActionPolicy/@eN concept); coupling would drag an unrelated dependency graph across both. rmcp 0.9 stdio server, THIN CLIENT over the daemon's Unix socket (not a second in-process embedding). Path-dep on neurobrowser for shared serde types ONLY. Reuse browser-mcp-rs's AgentResult<T> envelope, extended with policy_outcome/approval_id/reasons/risk_flags. Two-call approval via neurobrowser_ask/neurobrowser_approve. Ship v1 against the 7 working tools + ask + policy; gate the 5 not-yet-implemented tools.
- **Depends on:** Daemon Phase 1 wire methods + the new tools.list method. Must hold ONE long-lived socket connection per server process (or send explicit session_id) so policy.set customization isn't lost per call. Confirm rmcp version on crates.io before pinning.

### CLI
- **Decision:** NEW sibling crate cli/ (package neurobrowser-cli, [[bin]] name = neurobrowser), following the existing src-tauri path-dependency pattern — NOT a new top-level [workspace]. clap derive, blocking std::os::unix::net::UnixStream client (no async runtime needed), 1:1 verb-per-tool surface matching AGENT-SURFACE.md + the Tauri command surface. Reuses core serde types directly. neurobrowser serve spawns neurobrowser-headless as a child and persists the endpoint to ~/.neurobrowser/daemon.json. Script-friendly exit codes (BLOCKED=2, AwaitingApproval=3, TIMEOUT=4, transport=5). Never accepts an API key as an argv flag — env or --api-key-file only. Add cargo check --manifest-path cli/Cargo.toml to verify.sh.
- **Depends on:** Daemon Phase 1 wire methods (navigate/click/ask/session/workers all UNKNOWN_METHOD until the daemon lands). Method names chosen to be the exact RPC names so CLI and daemon converge, not drift.

## Phased build plan (ordered by dependency, not time)

### Phase 0 — Core contract hardening (library, in-crate)
Add RefMapEntry + build_ref_map(html) + ref_map field (#[serde(default)]) to PageSnapshot so both engines converge on one Rust type (fixes the serde-drops-ref_map bug even in Tauri). Add per-session `policy: Mutex<ActionPolicy>` + bounded `decision_log: VecDeque<PolicyDecision>` to SessionManager's SessionState with get/set/record/snapshot accessors + close_session + page.list. Add BrowserInterface::click_ref/type_text_ref/submit_form_ref (default-unsupported) + BrowserEngine/TauriBrowserRuntime impls + ClickRef/TypeRef/SubmitFormRef tools (wires the dead runtime.rs JS bridge). Add the NeuroBrowser facade. Derive JsonSchema on the contract set (activate schemars). Add effective_under_engine to ToolDefinition.
- _depends on:_ Nothing — this is the foundation everything else consumes.

### Phase 1 — Daemon wiring (THE foundation for all clients)
Replace SessionState with DaemonState + HeadlessEngineRegistry. Implement all JSON-RPC methods (session.*/page.*/navigate/snapshot[real]/tool.exec/list_tools/tools.list/ask/run[streaming]/approval.resolve/run.cancel/worker.*/policy.*[now per-session]/skills.get) as thin adapters over the reused core code paths, mirroring main.rs command shapes 1:1. Add Response.stream field, spawn_blocking wrapper at the RPC boundary. Fix policy.snapshot to actually return the decision history its doc comment promises.
- _depends on:_ Phase 0 (per-session policy/decision-log, ref_map, ref-based BrowserInterface methods, effective_under_engine).

### Phase 1b — Socket authz integration (parallel hardening task)
Tag each accepted connection with (uid,pid) from SO_PEERCRED; check conn_identity.owns(session_id) once before dispatch's method match. No per-handler changes because every method already takes session_id as a plain param.
- _depends on:_ Phase 0 per-session state (without it there is nothing meaningful to scope authz to — one client's agent/policy slot IS every other's today). Composes into Phase 1's dispatch.

### Phase 2 — Thin clients (CLI and MCP, parallelizable with each other)
CLI: cli/ crate — clap tree, blocking UDS/TCP client, human + --json formatters, exit-code mapping, serve/doctor/skills. MCP: neurobrowser-mcp repo — rmcp stdio server, daemon_client UDS helper, per-tool handlers, extended AgentResult envelope, AGENTS.md documenting the actuator boundary + wire-gate obligation. Both open the socket and speak the Phase 1 protocol.
- _depends on:_ Phase 1 wire contract (both are pure clients — non-functional until the daemon exposes the methods). Independent of each other.

### Phase 3 — Library polish + semver discipline
examples/ (single_page_ask, policy_gated_run, worker_fanout), crate-level //! docs naming the four surfaces, /// on every root re-export, prune root re-exports to one canonical path per symbol (documented breaking release per pre-1.0 minor=breaking policy), Cargo.toml metadata + version reconciliation, verify.sh += cargo doc --no-deps + cargo check for cli/ + optional cargo-public-api/semver-checks gate.
- _depends on:_ Phase 0 facade (docs/examples target it). Can overlap Phase 2. The root-reexport prune is breaking, so it lands as its own tagged release with a migration note.

### Phase 4 — Deferred / gated (real interactivity + missing tools)
Promote src-tauri/src/runtime.rs into a [lib] target so a future --tauri headless mode can reuse TauriBrowserRuntime (prerequisite: src-tauri has no [lib] today and mod runtime is private to main.rs). Implement the 5 spec-ahead-of-code tools (evaluate — no BrowserInterface method exists; get_attribute singular; wait_for(selector); extract_text; screenshot override in TauriBrowserRuntime). Only after this does daemon click/type/submit mutate real page state.
- _depends on:_ Phase 1 (daemon must exist to route to it). Explicitly out of scope for v1 of the four surfaces — v1 ships BrowserEngine read/navigate/ask with inert click/type, honestly surfaced via effective_under_engine.

## Backlog items this design subsumes

- Daemon-wiring gap: snapshot returning a hardcoded about:blank stub (headless.rs:336-348) → real BrowserEngine-backed snapshot; and no navigate/ask/tool-exec methods → full method set.
- Daemon single-shared-state audit gap: every connection sharing one global ActionPolicy/agent slot (headless.rs:106-113,223) → per-session isolation via SessionManager. This is also the prerequisite that makes the socket-authz work meaningful.
- Socket peer-cred authz task: reduced to a single narrow integration point (tag connection, check owns(session_id) once before dispatch) because every method now takes explicit session_id.
- NB-12 tool-profile scoping: absorbed via per-session ActionPolicy + list_tools/tools.list surfacing per-tool ToolRisk + effective_under_engine hint, so callers scope the tool surface per session instead of a process-global.
- CLI v0.2 / v0.1.1 stub: the SKILL.md neurobrowser-cli ask/click/snapshot --session auto stub and the feature-backlog 'neurobrowser skills get <topic>' item are both fulfilled by the cli/ crate + skills.get RPC.
- Credential-vault NB-3 direction: the CLI/daemon standardize on the existing provider env-var conventions (ANTHROPIC_API_KEY/OPENAI_API_KEY/etc. from provider_config_for) and the never-key-on-argv rule, giving NB-3 a single choke point.
- NB-18 EngineAdapter: the NeuroBrowser facade is deliberately generic over BrowserInterface, staying compatible with a future headless-webview backend rather than duplicating it.
- ref_map silently dropped by serde (JS builds it, PageSnapshot had no field) — a real latent bug in the Tauri path too, not just headless.
- Dead clickRef/typeTextRef/submitFormRef JS bridge in runtime.rs (zero Rust callers) — wired via BrowserInterface extension.
- policy.snapshot doc-vs-code mismatch (promises decision history, returns only policy) — fixed with the bounded per-session decision log.
- StreamingAgent/StreamEvent subsystem implemented+unit-tested but wired to no surface — now driven by daemon `run` and CLI --follow.
- schemars declared-but-dead dependency — activated as the MCP tool-schema generator from the same contract structs.

## Risks / open questions

- BrowserEngine's click/type_text/submit_form are permanent no-ops (browser/mod.rs:232-264): any daemon/MCP/CLI call to those tools under the default engine returns success:true with NO page mutation. Mitigation (effective_under_engine hint) only works if every client actually surfaces and honors it — otherwise callers are silently misled. Real interactivity is Phase 4, gated on the src-tauri [lib]-target refactor + a webview run loop.
- Moving ActionPolicy from process-global to per-session is a behavior change: any caller relying on policy.set applying process-wide breaks. Needs a default-session convenience concept or a migration note (open question: is session.create mandatory as the first call, or is there a no-session mode for simple CLI use?).
- reqwest::blocking inside BrowserEngine's async navigate means spawn_blocking is necessary but not sufficient — under high concurrency it still consumes blocking-pool threads 1:1 with in-flight navigations. A cleaner async-client rewrite of BrowserEngine is a larger, riskier change to a type with other callers; embedding the facade in an arbitrary external async runtime hits the same landmine and must be documented loudly.
- Streaming run cancellation: StreamingAgent::execute_stream has no cancellation hook today, so run.cancel (drop the mpsc receiver or thread a CancellationToken) may only take effect at the next tool-call boundary, not instantly.
- MCP has no universally-supported mid-call host-confirmation primitive; the two-call approve pattern is robust but relies on the calling model choosing to invoke neurobrowser_approve rather than silently retrying — needs an explicit tool-description nudge. Also confirm the exact rmcp crate version on crates.io before pinning (sources disagreed: 0.8.5 vs 0.16.0).
- Client-daemon connection lifetime coupling: if the MCP server opens a new socket connection per tool call, per-connection session state (and any policy.set customization) is lost. The MCP/CLI must hold one long-lived connection per process, or the daemon must key everything on explicit session_id — a concrete cross-task coordination point.
- 5 of the 12 AGENT-SURFACE.md tools (evaluate, get_attribute, wait_for(selector), extract_text, screenshot) have no working Rust path today (missing trait method, missing tool struct, or TauriBrowserRuntime not overriding the 'unsupported' default). Listing the full 12 before Phase 4 lands means 5 tools hardcode-error — worse UX than not listing them. Ship v1 against the 7 that work.
- The src-tauri [lib]-target refactor (Phase 4) touches build configuration shared with the desktop app binary; must be validated it doesn't change the desktop app's own build/behavior.
- No CI gate catches accidental public-API breakage until cargo-public-api/cargo-semver-checks is wired into verify.sh; until then the root-reexport prune and facade stability depend on manual changelog diligence. Pruning root re-exports is itself a breaking change for tests/consumers importing from root today — must land as a documented release, not silently.

## Appendix — per-area investigation notes

### daemon-wiring
**Summary:** The headless daemon (src-tauri/src/bin/headless.rs) currently dispatches only ping + policy.get/set/evaluate/snapshot; `snapshot` returns a hardcoded about:blank stub (lines 336-348), and `SessionState` (lines 91-172) is instantiated ONCE in `main()` and Arc-cloned into every connection, so every client shares one `ActionPolicy` and one unused `agent` slot — there is no real per-session isolation and no way to actually navigate or run the agent loop over the wire. The fix is to make the daemon a thin JSON-RPC front end over the SAME `SessionManager`/`ReActAgent`/`BrowserEngine` stack that `src-tauri/src/main.rs` already wires for the Tauri commands (session_id/page_id-scoped, per-session `ActionPolicy`, `execute_with_policy`/`execute_approved_tool` for ask/approve), add real `navigate`/`snapshot`/`tool.exec`/`list_tools`/`run` (streaming)/`approval.resolve`/`cancel` methods, and extend `PageSnapshot`/`BrowserInterface` with a real `ref_map` + ref-based click/type/submit — closing the currently-dead `clickRef`/`typeTextRef`/`submitFormRef` JS bridge in `src-tauri/src/runtime.rs` that no Rust code ever calls.

**Design:** ## 1. Two-engine model (state today, don't overclaim)

`BrowserEngine` (src/browser/mod.rs:86-149) is a **non-rendering** reqwest+scraper engine — its own doc comment at browser/mod.rs:8-10 says so. Its `click`/`type_text`/`submit_form`/`scroll_to`/`scroll_by` (lines 232-264) are literal no-op tracing logs; there is no live DOM to mutate. `TauriBrowserRuntime` (src-tauri/src/runtime.rs:579-762) is the only engine that actually mutates a page (via `runtime.click(selector)` etc. in the injected JS, runtime.rs:272-313).

The daemon must be honest about this split: with `BrowserEngine` (the only engine buildable without a display), `navigate`/`snapshot`/read-tools (`get_text`/`get_links`/`get_prices`/`get_tables`/`query_selector`) are REAL; `click`/`type`/`submit_form` tool calls will report success but do nothing to server state. Wiring a real interactive engine into the daemon means wiring `TauriBrowserRuntime`, which requires a structural prerequisite (see §6) — that's phase 2, not this wiring pass. `list_tools` should surface an `effective_under_engine` hint per tool so callers (ROSA, an MCP client) aren't misled.

## 2. Reuse SessionManager instead of re-inventing session/page bookkeeping

`src/session/mod.rs`'s `SessionManager` (11-174) + worker registry (176-319) already implements exactly the multi-tenant model the daemon needs: `create_session`/`create_page`/`get_page`/`close_page`/`spawn_worker`/`list_workers`/`get_worker`, returning `PageHandle { id, runtime_id, agent: Arc<ReActAgent> }` (session/mod.rs:321-326). `src-tauri/src/main.rs` already proves the pattern for Tauri commands: `browser_for_page` (main.rs:137-151) does `session_manager.get_page(session_id, page_id)` then wraps it in a fresh `TauriBrowserRuntime`. The daemon should do the identical thing, substituting a `HeadlessEngineRegistry: Mutex<HashMap<usize, Arc<BrowserEngine>>>` (new, small — analogous to `BrowserRuntimeRegistry` in runtime.rs:433-569 but without webview/viewport concerns) for `BrowserRuntimeRegistry`.

`DaemonState` (replaces today's single global `SessionState`, headless.rs:91-172):
```rust
struct DaemonState {
    session_manager: Arc<SessionManager>,
    engines: Arc<HeadlessEngineRegistry>,     // page_id -> Arc<BrowserEngine>
    tool_registry: Arc<ToolRegistry>,          // shared/immutable, same as today (headless.rs:103,111)
    pending_approvals: Arc<Mutex<HashMap<String, PendingApproval>>>, // mirrors main.rs:24,27-32
}
```
One `Arc<DaemonState>` is created in `main()` and `.clone()`'d per connection (cheap Arc clone, same as today at headless.rs:226), but **`ActionPolicy` moves off the daemon-global singleton and onto `SessionManager`'s per-session `SessionState`** (session/mod.rs:19-28 needs a new field `policy: Mutex<ActionPolicy>`, with `SessionManager::get_policy(session_id)`/`set_policy(session_id, policy)` accessors mirroring the existing `set_provider_config` pattern at session/mod.rs:159-173). This directly fixes the confirmed bug that today's single `SessionState::policy` (headless.rs:94) is shared by literally every connected client.

## 3. New JSON-RPC methods, mapped to existing code

All methods take `session_id`/`page_id` in `params` (mirrors main.rs's Tauri command signatures exactly):

| method | maps to | notes |
|---|---|---|
| `session.create` | `SessionManager::create_session()` (session/mod.rs:40-62) | exists today, never exposed over the wire |
| `session.list` | `SessionManager::list_sessions()` (session/mod.rs:113-124) | |
| `session.close` | **new** `SessionManager::close_session` | doesn't exist yet — only `close_page` does |
| `page.create` | `SessionManager::create_page(session_id)` (72-100) + `engines.insert(page_id, Arc::new(BrowserEngine::new(PageConfig::default())))` | engine-agnostic `runtime_id` field (session/mod.rs:90 `format!("page-runtime-{page_id}")`) is reused as-is |
| `page.close` | `SessionManager::close_page` (137-157) + `engines.remove(page_id)` | |
| `page.list` | **new**, iterate `SessionInfo` — SessionManager has no page-listing accessor today, only `page_count` | |
| `navigate` | `browser.navigate(url)` via `BrowserInterface` (browser/mod.rs:153-199) | reuses existing SSRF guard (browser/mod.rs:17-48) + http/https-only check; no new validation code needed |
| `snapshot` | `browser.snapshot()` (browser/mod.rs:266-279) via `enrich_snapshot` (mod.rs:304+) | REAL page state, replacing headless.rs:336-348's hardcoded stub; see §4 for ref_map |
| `tool.exec` | `ToolRegistry::get(name)` + `BrowserTool::execute(args, browser)` (tools/mod.rs:42-58, 196-198) | same arg-parsing shape headless.rs:322-335 (`policy.evaluate`) already uses — `HashMap<String,String>` from JSON |
| `list_tools` | `ToolRegistry::definitions()` (tools/mod.rs:207-209, already exists, never exposed) | add `effective_under_engine: bool` per definition (§1) |
| `ask` | `page.agent.execute_with_policy(prompt, &browser, &policy)` (agent/mod.rs:132-350), single-shot | mirrors main.rs:299-361 `ask` command 1:1 |
| `run` | same call, but **streamed** — see §5 | new: today nothing drives the agent loop from the daemon at all |
| `approval.resolve` | `page.agent.execute_approved_tool(run_id, approval_id, tool_call, &browser, approved, message)` (agent/mod.rs:352-451) | mirrors main.rs:425-451 `submit_approval`; `PendingApproval` struct copied verbatim from main.rs:27-32 |
| `run.cancel` | mirrors main.rs:453-475 `cancel_agent_run` (removes from `pending_approvals`, synthesizes `AgentRunResult{status:Cancelled,...}`) | |
| `worker.spawn` / `worker.list` / `worker.get` | `SessionManager::spawn_worker`/`list_workers`/`get_worker` (session/mod.rs:189-229) | exists, unused by any command today (not even Tauri exposes `spawn_worker`) |
| `policy.get`/`policy.set`/`policy.evaluate`/`policy.snapshot` | same as today (headless.rs:302-335, 349-356) | **but** now keyed by `session_id` via `SessionManager::get_policy`/`set_policy`, not the shared global |

## 4. `snapshot` must actually carry a `ref_map` (currently silently dropped, even in Tauri)

Confirmed gap: the injected JS `runtime.snapshot()` (runtime.rs:174-205) builds `ref_map: collectRefMap()` (runtime.rs:203, 356-383), but Rust's `PageSnapshot` struct (tools/mod.rs:105-121) has **no `ref_map` field**. `TauriBrowserRuntime::snapshot()` (runtime.rs:752-757) does `serde_json::from_value::<PageSnapshot>` on that JS object — serde silently drops unknown fields, so `ref_map` is thrown away today, in the desktop app too, not just headless.

Fix (needed regardless of the daemon, but required to satisfy "snapshot [REAL, with ref_map]"):
1. Add `pub ref_map: HashMap<String, RefMapEntry>` (with `#[serde(default)]` for back-compat) to `PageSnapshot` (tools/mod.rs:105-121), where `RefMapEntry { tag, id, classes, text, selector, xpath }` mirrors the JS shape at runtime.rs:369-376.
2. `TauriBrowserRuntime::snapshot()` then round-trips the JS-collected ref_map for free (no logic change, just the added field).
3. For `BrowserEngine` (no live DOM), add `build_ref_map(html: &str) -> HashMap<String, RefMapEntry>` in browser/mod.rs using the same interactive-element selector list as the JS (`a[href], button, input, textarea, select, [role="button"], [role="link"], [role="textbox"], [tabindex]`) via `scraper::Selector`, called from `enrich_snapshot()` (browser/mod.rs:304+) so both engines converge on one Rust type.

## 5. Ref-based tool exec: wire the dead JS bridge

`runtime.clickRef`/`typeTextRef`/`submitFormRef` (runtime.rs:229-270) exist in the injected JS but **no `BrowserInterface` method and no `BrowserTool` calls them** — confirmed dead code today. Fix: extend `BrowserInterface` (tools/mod.rs:60-103) with default-unsupported methods `click_ref`/`type_text_ref`/`submit_form_ref` (same pattern as the existing `keypress`/`screenshot` defaults at tools/mod.rs:72-90), implement them in `TauriBrowserRuntime` (call the existing JS), and in `BrowserEngine` by resolving `ref` through the last snapshot's `ref_map[ref].selector` then delegating to the existing selector-based methods (which remain log-only per §1). Add corresponding `ClickRefTool`/`TypeRefTool`/`SubmitFormRefTool` to `default_tool_registry()` (browser/mod.rs:282-302) so `tool.exec` can address elements by `@eN` the same way an agent's tool-calling loop does internally.

## 6. Streaming `run` over the line protocol

`ReActAgent` already implements `StreamingAgent::execute_stream` (agent/mod.rs:560-641), forwarding `AgentRunEvent`s (policy.rs:102-142) as `StreamEvent`s (agent/streaming.rs:20-63) over an `mpsc::Sender<StreamEvent>`, with `execute_with_timeout` (streaming.rs:78-104) already available. Wire protocol extension (backward compatible): add an optional `stream: bool` field (`#[serde(skip_serializing_if = "..:not")]`, default false) to the existing `Response` struct (headless.rs:53-61). For `run`: spawn `execute_stream` with a bounded `mpsc` channel, forward every `StreamEvent` as `Response{id: request.id, ok:true, stream:true, result:{event:...}}` lines as they arrive, then a final `Response{id, ok:true, stream:false, result:{final AgentRunResult}}` (or `stream:false` terminal error on failure). Every existing single-shot method (`ping`, `navigate`, `policy.*`) is unaffected — they just never set `stream`. `run.cancel` drops the sender side of the channel (or a `CancellationToken` alongside it) to stop `execute_stream` mid-flight — new work, since `StreamingAgent` has no built-in cancel hook today.

## 7. Composing with the in-flight peer-cred authz work

Because every method now takes an explicit `session_id` (created via `session.create`) instead of relying on connection identity implicitly, the peer-cred/per-connection-authz work has a single, narrow integration point: tag each accepted connection with `(uid, pid)` from `SO_PEERCRED` at `handle_connection` (headless.rs:264-297) entry, and check `conn_identity.owns(session_id)` once, before the `match request.method.as_str()` in `dispatch()` (headless.rs:299-363) — no changes needed inside individual method handlers, since they already receive `session_id` as a plain param. This also closes the audit gap noted in the task: today's single shared `SessionState` (headless.rs:106-113) means peer-cred work would have nothing meaningful to scope *to* (one client's `agent`/`policy` slot IS every other client's); moving to per-session state is a prerequisite for that authz work to mean anything, not just a nice-to-have alongside it.

## 8. `policy.snapshot` doesn't match its own doc comment

headless.rs:349-356's comment says it should "capture the current policy + the last 5 policy decisions", but the code only returns the policy (identical to `policy.get`). Fix as part of this pass: add a bounded ring buffer of the last N `PolicyDecision`s per session (`SessionManager`'s per-session state, alongside the new `policy: Mutex<ActionPolicy>` field), populated wherever `ActionPolicy::evaluate` (policy.rs:156-331) is called from `tool.exec`/`ask`/`run`, and have `policy.snapshot` actually return both.

## 9. Threading caveat: BrowserEngine blocks on reqwest::blocking

`BrowserEngine::navigate` (browser/mod.rs:162 `self.http_client.get(url).send()`) uses `reqwest::blocking::Client` inside an `async fn`. In a multi-connection tokio daemon this stalls whichever tokio worker thread picks up that request for the HTTP round-trip. Fix at the daemon boundary only (don't touch BrowserEngine's existing sync API, which other callers may depend on): wrap every `Arc<BrowserEngine>` call site in the daemon's RPC handlers with `tokio::task::spawn_blocking`, so one slow `navigate` from client A can't starve client B's `ping`/`policy.get`.

## 10. Tauri-engine-in-daemon is blocked on a structural prerequisite (flag, don't hand-wave)

The doc comment at headless.rs:11-12 promises a future `--tauri` flag reusing `TauriBrowserRuntime`. Today that's impossible without a refactor: `src-tauri/Cargo.toml` declares **no `[lib]` target** (only two `[[bin]]`s: the default `src/main.rs` and `neurobrowser-headless` at src/bin/headless.rs, confirmed via `grep -n "\[lib\]\|\[\[bin\]\]"`), and `mod runtime;` is declared inside `main.rs` (main.rs:3) — private to that binary's compilation unit. `src/bin/headless.rs` is a separate crate root and cannot `use crate::runtime::TauriBrowserRuntime` as written. Prerequisite: promote `runtime.rs` (+ `BrowserRuntimeRegistry`, `TauriBrowserRuntime`, `create_runtime_page`, etc.) into a `[lib]` target in src-tauri/Cargo.toml so both bins import `neurobrowser_tauri::runtime::*`. Even after that, a headless (no-display) process running a real Tauri webview needs an actual windowing/webview subsystem (WKWebView needs an NSApplication run loop on macOS) — running the JSON-RPC accept loop as a background tokio task while Tauri's event loop owns the main thread is workable but is materially more work than this wiring pass. Scope this as phase 2, gated on the lib-target refactor landing first.

## Audit gaps this design closes
1. `snapshot` returning a hardcoded `about:blank` stub (headless.rs:336-348) → real `BrowserEngine`-backed snapshot.
2. Every connection sharing one global `ActionPolicy`/`agent` slot (headless.rs:106-113, 223-227) → per-session isolation via `SessionManager`.
3. `SessionManager`'s `create_session`/`create_page`/`spawn_worker` (session/mod.rs) existing but never reachable except through the Tauri desktop UI → exposed over the wire.
4. `ref_map` computed in JS but silently dropped by serde (runtime.rs:203 vs tools/mod.rs:105-121) → real field + Rust-side computation for `BrowserEngine`.
5. `clickRef`/`typeTextRef`/`submitFormRef` JS with zero Rust callers (runtime.rs:229-270) → wired via `BrowserInterface` extension.
6. No way to actually run the agent loop, approve/deny, or cancel from outside the Tauri app (`ask`/`submit_approval`/`cancel_agent_run` are Tauri-only commands in main.rs) → daemon `ask`/`run`/`approval.resolve`/`run.cancel`.
7. `policy.snapshot`'s doc comment promising decision history it doesn't deliver (headless.rs:349-356) → bounded per-session decision log.
8. `BrowserEngine`'s blocking HTTP client silently degrading daemon concurrency → `spawn_blocking` wrapper at the RPC boundary.

**New work:**
- DaemonState (session_manager, engines registry, shared tool_registry, pending_approvals) replacing the single global SessionState in headless.rs — new struct in headless.rs, ~60 lines, modeled on src-tauri/src/main.rs's AppState (main.rs:20-32)
- HeadlessEngineRegistry: page_id -> Arc<BrowserEngine> — new small struct in headless.rs or a new src-tauri/src/headless_registry.rs, modeled on BrowserRuntimeRegistry (runtime.rs:433-569) minus viewport/webview concerns
- Per-session ActionPolicy + bounded PolicyDecision log on SessionManager's SessionState — add `policy: Mutex<ActionPolicy>` and `decision_log: Mutex<VecDeque<PolicyDecision>>` fields to session/mod.rs's private SessionState (session/mod.rs:19-28) + get_policy/set_policy/record_decision/decision_snapshot accessors, mirroring set_provider_config (session/mod.rs:159-173)
- SessionManager::close_session and page.list accessor — two small new methods on SessionManager, session/mod.rs
- RefMapEntry type + build_ref_map(html) + PageSnapshot.ref_map field — new struct + fn in browser/mod.rs; #[serde(default)] field added to tools/mod.rs PageSnapshot (105-121); called from enrich_snapshot (mod.rs:304+)
- BrowserInterface::click_ref/type_text_ref/submit_form_ref (default-unsupported) + BrowserEngine and TauriBrowserRuntime impls + ClickRefTool/TypeRefTool/SubmitFormRefTool — trait methods added to tools/mod.rs:60-103 (same default-impl pattern as keypress/screenshot); ~3 new BrowserTool impls registered in default_tool_registry (browser/mod.rs:282-302); TauriBrowserRuntime impls call the already-existing runtime.clickRef/typeTextRef/submitFormRef JS (runtime.rs:229-270)
- New JSON-RPC methods: session.create/list/close, page.create/close/list, navigate, snapshot (real), tool.exec, list_tools, ask, run (streaming), approval.resolve, run.cancel, worker.spawn/list/get — match arms in headless.rs dispatch(), each a thin adapter calling the reused code paths above
- Response.stream: bool field for the streamed `run` method — one new optional field on headless.rs's Response struct (53-61), #[serde(skip_serializing_if)] for back-compat
- spawn_blocking wrapper around BrowserEngine calls at the RPC boundary — small helper in headless.rs wrapping engine.navigate/snapshot/etc. calls used from async dispatch()
- (Phase 2, gated) promote src-tauri/src/runtime.rs into a [lib] target so a future --tauri headless mode can reuse TauriBrowserRuntime from src/bin/headless.rs — Cargo.toml [lib] section + path adjustments; explicitly out of scope for this wiring pass, called out as a prerequisite

**Open questions:**
- Should ActionPolicy default to a brand-new 'default session' when a client sends session_id-less requests (for the simplest CLI use case), or should session.create always be mandatory as the very first call? The Tauri app's single global policy today suggests some callers may want a no-session convenience mode.
- Does the peer-cred authz work (in flight, per the task) want session ownership to be 1:1 with the connecting uid, or should multiple connections (e.g. a supervisor process + a worker process both owned by ROSA) be allowed to attach to the same session_id? This determines whether `conn_identity.owns(session_id)` is a strict map or an ACL.
- Is a Tauri-backed headless engine (§10) actually wanted for v1, or is BrowserEngine-only (read/navigate/ask, inert click/type) an acceptable v1 scope for the four composable surfaces, with real interactivity deferred? This changes how much of the design doc's 'click'/'type' ambitions are real vs aspirational in the first release.
- Should the bounded per-session PolicyDecision log (§8) be persisted to disk for the Phase F audit log, or is an in-memory ring buffer sufficient given the daemon is expected to be long-running but not durable across restarts?

### mcp-wrapper
**Summary:** Build a NEW sibling crate `neurobrowser-mcp` (own top-level repo, alongside browser-mcp-rs/macos-mcp-rs) — an rmcp stdio server that is a THIN CLIENT over the wired headless daemon's Unix-socket JSON-RPC protocol, not a second in-process embedding of BrowserEngine/ReActAgent. It exposes NeuroBrowser's ref-based tool surface (docs/AGENT-SURFACE.md) as MCP tools, reuses the existing ToolRisk/ActionPolicy types verbatim for MCP tool annotations and a two-call approval flow, and reuses the portfolio's AgentResult&lt;T&gt; envelope convention from browser-mcp-rs. Do NOT extend browser-mcp-rs — it is a different capability (`actuator.browser`, generic chromiumoxide/CDP automation of any site) with an explicit boundary rule against cross-contamination; NeuroBrowser is a different product (its own BrowserEngine/TauriBrowserRuntime, ReActAgent, ActionPolicy, ref/@eN model) with no dependency relationship to chromiumoxide at all.

**Design:** **1. Reuse-vs-new-crate decision.** Read `/Users/jamespustorino/code/browser-mcp-rs/{Cargo.toml,AGENTS.md,src/{envelope.rs,capability.rs}}`. browser-mcp-rs is `actuator.browser`: it drives a real Chromium over CDP (chromiumoxide) with a generic click/type/evaluate_js/screenshot surface and zero concept of NeuroBrowser's ReActAgent, ActionPolicy, PolicyOutcome, or @eN ref_map. Its own AGENTS.md (lines 27-37) states the boundary rule verbatim: 'Clean tool boundary... Two capability slugs, two risk surfaces, no cross-contamination in a client's tool list' and 'Different platforms... Coupling them would drag the browser server onto a macOS-only build' (analogous argument applies: coupling would drag an unrelated chromiumoxide/operator dependency graph into NeuroBrowser's surface, and vice versa). Verdict: new crate `neurobrowser-mcp`, own repo, own lifecycle — matching the established one-repo-per-MCP-actuator pattern (macos-mcp-rs, browser-mcp-rs).

**2. Thin client over the daemon, not the library.** The task brief asks to decide 'thin client over the wired daemon, or directly over the library'. Verdict: over the daemon (`src-tauri/src/bin/headless.rs`). `SessionManager` (`src/session/mod.rs` lines 11-17, 40-90) and the worker registry (`src/agent/worker.rs`) already own multi-tab/multi-worker state per session, and the desktop Tauri app (`src-tauri/src/main.rs`) drives the SAME session/page/agent objects a human is watching in a live webview. If `neurobrowser-mcp` embedded `ReActAgent`/`BrowserEngine` in-process it would fork a second, invisible browser session — defeating the 'AI-native browser with human-in-the-loop' premise and violating the goal's own 'ONE core, thin clients' framing. The daemon (once wired per the sibling daemon-wiring task) IS the one core process; MCP/CLI are peer JSON-RPC clients over its Unix socket (`NEUROBROWSER_SOCKET`, newline-delimited JSON, `headless.rs` lines 174-233). This keeps `neurobrowser-mcp`'s Cargo.toml lean: `rmcp` (server+macros+transport-io), `tokio` (net+io-util for a UDS client), `serde_json`, `schemars`; it depends on `neurobrowser = { path = "/Users/jamespustorino/code/neurobrowser" }` ONLY for already-public serde types (`ActionPolicy`, `AutonomyLevel`, `PolicyOutcome`, `RiskFlag`, `ToolDefinition`, `ToolRisk`, `RiskLevel`, `ToolAction`, `PageSnapshot`, re-exported from `src/lib.rs` line 31) — never `BrowserEngine`/`ReActAgent` themselves.

**3. Contract this crate needs FROM the daemon-wiring task.** Today `headless.rs` dispatch (lines 299-363) only implements `ping`, `policy.get/set/evaluate`, and a hardcoded-stub `snapshot` (`about:blank`, empty `ref_map`) — no `navigate`, `click`, `type_text`, `submit_form`, `query_selector`, `get_text`, `evaluate`, `get_attribute`, `wait_for`, `extract_text`, `screenshot`, or `ask`. `docs/AGENT-SURFACE.md` (spec-of-record, lines 7-9, 43-181) already defines these 12 method names + the `[@e1,@e2,...]` ref model + the `{ok:false, pending_approval_id, reasons}` shape for blocked/approval-required calls — the MCP wrapper's tool schemas are written directly against that doc. I additionally need one NEW daemon method not yet in the spec: `tools.list` returning `Vec<ToolDefinition>` (type exists, `src/tools/contracts.rs` lines 99-123; `ToolRegistry::definitions()` already produces it, `src/tools/mod.rs` lines 207-209) so the MCP wrapper generates its MCP `Tool.inputSchema`/annotations from the SAME risk metadata the daemon's `policy.evaluate` uses, instead of a hand-maintained copy that will drift.

**4. Concrete gap: the spec is ahead of the Rust implementation for 5 of the 12.** Cross-checking `docs/AGENT-SURFACE.md` against `default_tool_registry()` (`src/browser/mod.rs` lines 282-300+, 17 `BrowserTool` impls, not 12) and `TauriBrowserRuntime`'s `impl BrowserInterface` (`src-tauri/src/runtime.rs` lines 660-762): `navigate`, `click`, `type_text`, `submit_form`, `query_selector`, `get_text`, `scroll_to/scroll_by`, `keypress`, `back/forward/reload` all have a working Rust path today. But: (a) `evaluate(script)` — the JS runtime implements it (`runtime.rs` lines 340-348) but `BrowserInterface` (`src/tools/contracts.rs` lines 61-103) has NO `evaluate` trait method at all, unreachable from Rust; (b) `get_attribute(ref,name)` singular — only `get_attributes(selector)` plural exists (contracts.rs line 65), no registered tool; (c) `wait_for(selector,timeout_ms)` — the registered `WaitTool` only calls `wait_for_navigation()` with no args, doesn't match the spec; (d) `extract_text(ref,structured)` — no such tool struct exists; (e) `screenshot(viewport?)` — `ScreenshotTool` is registered (risk Low) but `TauriBrowserRuntime` never overrides the trait's `screenshot()` default, which unconditionally returns `Err("screenshot is not supported by this browser")` (contracts.rs lines 76-78) — non-functional even in the real webview path today. Recommendation: ship v1 against the 7 that work + `ask` + `policy.get/set`, gate the other 5 behind daemon support landing, and spin off a fix task against `src/tools/contracts.rs` + `src/browser/mod.rs` + `src-tauri/src/runtime.rs`.

**5. rmcp wiring pattern** (verified against the official `modelcontextprotocol/rust-sdk` README, and the working `rmcp = "0.9"` pin already proven by `browser-mcp-rs/Cargo.toml` line 9): `#[tool_router(server_handler)]` on the server struct's impl block, `#[tool(name="...", description="...")]` per method taking `Parameters<T: JsonSchema + Deserialize>`, `stdio()` transport, `Server.serve(stdio()).await?.waiting().await?`. Each tool method opens (or reuses a pooled) UDS connection to `$NEUROBROWSER_SOCKET`, writes one JSON-RPC request line, reads one response line, translates into an MCP `CallToolResult`.

**6. MCP tool list (v1):** `neurobrowser_snapshot {url_or_ref?}` → daemon `snapshot`, readOnlyHint:false (may navigate), all autonomy levels. `neurobrowser_navigate {url}` → `navigate`, risk `ToolAction::Navigate,Medium` (mod.rs:517), openWorldHint:true; ReadOnly allow, Assisted allow same-domain/RequireApproval cross-domain (policy.rs:295-310), HighAutonomy allow. `neurobrowser_click {ref}` → `click`, risk `Click,Medium` (mod.rs:834); ReadOnly Block, Assisted RequireApproval, HighAutonomy allow. `neurobrowser_type_text {ref,text}` → `type_text`, risk `Type,High,sensitive` (mod.rs:873), destructiveHint:true; RequireApproval at ReadOnly/Assisted (sensitive-arg short-circuit, policy.rs:249-258 fires regardless of level), HighAutonomy allow (redacted in audit only). `neurobrowser_submit_form {ref}` → `submit_form`, risk `Submit,High,externally_visible` (mod.rs:1009); RequireApproval at ALL levels including HighAutonomy (policy.rs:312-327 always-gates Submit). `neurobrowser_query_selector {selector}` and `neurobrowser_get_text {ref}` → Read/Low, readOnlyHint:true, all levels. `neurobrowser_wait_for {selector?,timeout_ms?}` and `neurobrowser_screenshot {viewport?}` → Phase 2 (gap 4c/4e). `neurobrowser_evaluate {script}` → Phase 2 (gap 4a), must default to `Destructive/Critical` risk until classified (mirrors the daemon's own unknown-tool fallback, headless.rs:144-148). `neurobrowser_ask {prompt}` → new `ask` daemon method mirroring `src-tauri/src/main.rs` `ask` (lines 299-361); sub-actions individually gated, `AwaitingApproval` surfaces an `approval_id`. `neurobrowser_approve {run_id,approved,message?}` → new daemon method mirroring `submit_approval`/`execute_approved_tool` (main.rs:426-451). `neurobrowser_get_policy`/`neurobrowser_set_policy` → existing `policy.get`/`policy.set` (headless.rs:302-321); `set_policy` treated as sensitive-by-convention at the MCP layer since it's a standing-configuration change.

**7. Approval mapping.** MCP has no universally-supported mid-call host-confirmation primitive (elicitation exists in-spec but client support is inconsistent), so this design reuses the SAME two-call pattern the Tauri UI already implements (`ask` → `AwaitingApproval`+`approval_id` → `submit_approval`, main.rs:299-361,426-451): a gated MCP tool call returns `ok:false` with `policy_outcome:"require_approval"`, `approval_id`, and `reasons` (verbatim from `PolicyDecision`, policy.rs:59-65) instead of executing; the calling agent must explicitly invoke `neurobrowser_approve` — a visible, auditable second tool call rather than a silent client dialog, needing no new MCP capability.

**8. Response envelope.** Reuse `browser-mcp-rs`'s `AgentResult<T>` shape (`envelope.rs` lines 16-31) verbatim for `ok`/`data`/`unavailable`/`error`, extended with the fields NeuroBrowser's finer per-call policy already produces (browser-mcp-rs is statically Medium always, capability.rs lines 20-23 — no per-call policy exists there): add `policy_outcome: Option<String>`, `approval_id: Option<String>`, `reasons: Vec<String>`, `risk_flags: Vec<String>`, all serialized straight from `PolicyDecision`/`AgentRunResult` (policy.rs lines 59-153) with zero new logic. New capability slug `web.neurobrowser`, risk baseline Medium (type_text/submit_form mutate — port the same 'never drop below Medium while any tool mutates' invariant test from capability.rs lines 33-48).

**New work:**
- New top-level repo `neurobrowser-mcp` (Cargo.toml pinning rmcp 0.9, tokio net+io-util, serde_json, schemars; path-dep on neurobrowser for shared serde types only) — rmcp stdio server: main.rs thin edge, daemon_client.rs UDS request/response helper, tools.rs per-tool MCP handlers
- AGENTS.md for neurobrowser-mcp documenting the actuator-boundary rule against browser-mcp-rs and the wire-gate obligation for mutating tools — doc, ~60-80 lines, same structure as browser-mcp-rs/AGENTS.md
- Extended AgentResult<T>-style envelope with policy_outcome/approval_id/reasons/risk_flags fields — new struct in neurobrowser-mcp/src/envelope.rs, ported from browser-mcp-rs's envelope.rs plus From<PolicyDecision>/From<AgentRunResult> mappings
- New daemon method tools.list (returns Vec<ToolDefinition> from ToolRegistry::definitions()) — one dispatch arm added to headless.rs's match — ask against the daemon-wiring task, not built here
- neurobrowser_approve MCP tool mirroring submit_approval/execute_approved_tool — handler calling a new daemon approve method (parallel ask to the daemon-wiring task)

**Open questions:**
- Should neurobrowser_set_policy (a standing-configuration change) require its own approval gate at the MCP layer, or is per-tool-call gating via ActionPolicy sufficient?
- Does the daemon-wiring task intend session_id to be explicit in every JSON-RPC method (multi-tenant daemon) or implicit-per-connection (current headless.rs shape)? This decides whether neurobrowser-mcp needs one persistent socket connection per server process or can open one per call.
- Should neurobrowser-mcp also expose worker-registry tools (list_workers/get_worker/create_worker from src/session/mod.rs + src/agent/worker.rs) for multi-tab fan-out, or stay scoped to single-page ask/navigate/click in v1?
- Confirm exact current rmcp crate version on crates.io before pinning (WebFetch gave inconsistent numbers across two sources).

### cli
**Summary:** Design for `neurobrowser` (bin name `neurobrowser`), a thin clap-based CLI shipped as a new sibling crate `cli/` (parallel to `src-tauri/`, same pattern the repo already uses for the headless binary). It speaks the exact newline-delimited JSON-RPC protocol defined in src-tauri/src/bin/headless.rs and reuses the daemon's own serde types (ActionPolicy, AgentRunResult, WorkerSummary/WorkerSnapshot) so wire shapes cannot drift from the lib. Command surface is 1:1 with (a) the 12-tool AGENT-SURFACE.md contract and (b) the already-implemented Tauri command surface in src-tauri/src/main.rs — both of which the daemon-wiring task is expected to expose as new JSON-RPC methods (navigate/snapshot/ask/click/etc. do not exist on the wire yet; only ping/policy.get/policy.set/policy.evaluate/snapshot(stub)/policy.snapshot do today, per headless.rs:299-363).

**Design:** 
## Grounding (files read)

- `src-tauri/src/bin/headless.rs` — the only JSON-RPC server today. Wire format: newline-delimited `{id, method, params}` → `{id, ok, result|error:{code,message}}` (lines 46-67, 264-297). `dispatch()` (lines 299-363) currently handles only `ping`, `policy.get`, `policy.set`, `policy.evaluate`, `snapshot` (hardcoded `about:blank` stub, lines 336-348), `policy.snapshot`; everything else returns `UNKNOWN_METHOD`. Socket path comes from env `NEUROBROWSER_SOCKET` (line 180), falls back to a per-PID temp path, then to TCP printing `NEUROBROWSER_LISTENING=tcp://…` on stdout (lines 200-206). One shared `SessionState`/`ActionPolicy` per daemon process today, not per-connection (line 223) — the CLI must not assume session isolation until the parallel peer-cred/session-state task lands.
- `docs/AGENT-SURFACE.md` (261 lines) — the 12-tool contract (`snapshot`, `click`, `type_text`, `submit_form`, `query_selector`, `evaluate`, `navigate`, `get_text`, `get_attribute`, `wait_for`, `extract_text`, `screenshot`), the ref model (`@e1`…), the 3 autonomy levels, and the 5 error codes (`TIMEOUT`, `BLOCKED`, `NOT_FOUND`, `EVAL_ERROR`, `NAVIGATION_FAILED`, `INTERNAL`). This is the spec-of-record the CLI's per-tool subcommands must match verbatim.
- `SKILL.md` (lines 80-90) already stubs the intended CLI invocation shape (`neurobrowser-cli ask --session auto --prompt "..."`, `click --session auto --ref @e3`, `snapshot --session auto`) and explicitly defers `neurobrowser-cli` to "v0.1.1" — this design fulfills that stub.
- `docs/goals/feature-backlog-2026-07-20.md:145` — existing backlog line: "Add a `neurobrowser skills get <topic>` CLI / daemon RPC rendering the current tool surface + ref-model + policy semantics; checked-in SKILL.md becomes a pointer." Folded into the design below as `neurobrowser skills get <topic>`.
- `src-tauri/src/main.rs` — the real, already-implemented desktop command surface that the daemon needs to grow into: `create_session`(171), `create_page`(176), `navigate`(243), `get_page_snapshot`(278), `ask`(300), `get_action_policy`/`set_action_policy`(364,373), `start_agent_run`(383), `submit_approval`(426), `cancel_agent_run`(454), `list_workers`(478), `get_worker`(489), `close_page`(501), `list_sessions`(512), `browser_reload`/`back`/`forward`(526,541,556), `validate_url`(582), `set_provider`(611). Provider env-var conventions (`OPENAI_API_KEY`/`OPENAI_MODEL`, `ANTHROPIC_API_KEY`/`ANTHROPIC_MODEL`, `OLLAMA_BASE_URL`/`OLLAMA_MODEL`, `CUSTOM_PROVIDER_API_KEY`/`_BASE_URL`/`_MODEL`) are defined at `provider_config_for` (lines 97-135) and `provider_type_from_slug` (78-86) — the CLI's `provider set` reuses these exact names rather than inventing new ones.
- `src/agent/policy.rs` — `ActionPolicy` (37-44: `autonomy_level`, `allowed_domains`, `denied_domains`, `denied_tools`, `approval_required_tools`, `block_prompt_injection`), `AgentRunResult` (145-153), `AgentRunStatus` (94-100), `PolicyDecision`/`PolicyOutcome` (16-20, 59-65) — these are the exact serde types the CLI deserializes daemon responses into (no hand-rolled duplicate structs).
- `src/agent/worker.rs` — `WorkerSpec` (24-38: name/goal/policy/max_iterations/pinned_page_id), `WorkerSummary` (65-74), `WorkerSnapshot` (78-83), `WorkerStatus` (86-94) — backing types for `workers create|list|get`.
- Root `Cargo.toml` — `neurobrowser` is a plain lib crate (`src/lib.rs`), no `[workspace]` table. `src-tauri/Cargo.toml` is a sibling crate depending on the lib via `path = ".."` (line: `neurobrowser = { path = "..", features = [] }`) and defines its own `[[bin]]` (`neurobrowser-headless`, gated by `required-features = ["headless"]`). This is the precedent the new `cli/` crate follows — not a new top-level `[workspace]`, which would be a much bigger structural change than this task's scope.
- `verify.sh` — runs `cargo fmt`/`clippy`/`test` at the root, then a **separate** `cargo check --manifest-path src-tauri/Cargo.toml`. A new `cli/` crate needs its own line added the same way (`cargo check --manifest-path cli/Cargo.toml` / `cargo build --release --manifest-path cli/Cargo.toml`).
- External docs fetched (not guessed): `docs.rs/clap/latest/clap/_derive` (derive `Parser`/`Subcommand`, `#[arg(global = true)]` for cross-subcommand flags) and `github.com/vercel-labs/agent-browser` README + SKILL.md (fetched directly) — real, unrelated to the `docs/goals/feature-backlog-2026-07-20.md:9` note that a *different*, dormant, 2-commit `AIAnytime/agent-browser` repo is a pure name collision with no code lineage to NeuroBrowser. I did **not** conflate the two; all CLI-ergonomics borrowing below cites the real `vercel-labs/agent-browser` (Rust CLI, client-daemon architecture, `@eN` refs, `--json`, `diff`, `doctor`, `skills get`).

## Crate layout

New sibling crate `cli/Cargo.toml` (package `neurobrowser-cli`, `[[bin]] name = "neurobrowser"`):
```toml
[dependencies]
neurobrowser = { path = ".." }   # reuse ActionPolicy, AgentRunResult, WorkerSummary, etc.
clap = { version = "4", features = ["derive", "env"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
uuid = { version = "1", features = ["v4"] }
```
The client transport is a small blocking `std::os::unix::net::UnixStream` (or `std::net::TcpStream` fallback) request/reply loop — one line out, one line back — matching the daemon's own newline-delimited framing (headless.rs 264-297) exactly. No async runtime needed in the CLI binary itself (single in-flight request per invocation); this keeps `cargo install --path cli` fast and dependency-light, mirroring why `headless.rs` itself stays "platform-portable and dependency-light" (headless.rs:8-10).

## Command surface

Global flags (clap `#[arg(global = true)]`, following the documented `Parser`+`Subcommand`+`global=true` pattern):
- `--socket <path>` (env `NEUROBROWSER_SOCKET` — **reuses the identical env var name headless.rs already reads at line 180**, so `neurobrowser serve` and the CLI client agree with zero translation)
- `--tcp <host:port>` (fallback transport, mirrors headless.rs's own TCP fallback path)
- `--json` (machine-readable: print the raw `result`/`error` JSON envelope instead of a human-formatted view — same flag name and same "opt-in machine mode, human mode by default" convention as `vercel-labs/agent-browser`'s `--json`)
- `--timeout-ms <n>` (per-call socket read timeout)
- `-v/--verbose`, `-q/--quiet`
- `--config <path>` (optional TOML/JSON file, lowest precedence: config file < env < flags — same layering `agent-browser` documents for its own config)

Subcommands, grouped by what they map to:

1. **Daemon lifecycle**
   - `neurobrowser serve [--socket PATH] [--tcp PORT] [--daemon-bin PATH]` — spawns `neurobrowser-headless` (search `$PATH`, or `--daemon-bin` override) as a child process, captures its `NEUROBROWSER_LISTENING=...` stdout line (headless.rs:202,208), and persists the resolved endpoint to `~/.neurobrowser/daemon.json` so later CLI invocations in new shells don't need `--socket` re-passed. This keeps `serve` a thin spawner over the existing binary rather than reimplementing the listener — daemon hardening work happens once, in headless.rs, not twice.
   - `neurobrowser ping` — wraps the existing `ping` method (headless.rs:301) as a connectivity check.
   - `neurobrowser doctor` — `ping` + protocol/version sanity (checks the daemon answers `policy.get` with a well-formed `ActionPolicy`), modeled on `agent-browser doctor`.

2. **Browser tool surface** (1:1 with the 12 tools in AGENT-SURFACE.md — each becomes a first-class verb, matching `agent-browser`'s per-verb style like `click`/`fill`/`type`, rather than one generic passthrough):
   - `navigate <url> [--session ID] [--page N]`
   - `snapshot [--session ID] [--page N]`
   - `click <ref>`, `type <ref> <text>`, `submit <ref>` — `type_text`/`submit_form` per AGENT-SURFACE.md §3-4
   - `query <selector>` — `query_selector` §5
   - `eval <script>` — `evaluate` §6 (documented as sandboxed to page origin; CLI surfaces that constraint in `--help`, doesn't try to work around it)
   - `get-text <ref>`, `get-attr <ref> <name>` — §8-9
   - `wait-for <selector> [--timeout-ms N]` — §10
   - `extract <ref> [--structured]` — §11
   - `screenshot [--out FILE] [--width N] [--height N]` — §12, decodes `base64_png` to a file instead of dumping base64 to a terminal
   - Plus a generic escape hatch `neurobrowser tool <name> --arg k=v [--arg k=v ...]` for any tool not yet given a dedicated verb (forward-compatible with new tools added to `ToolRegistry`/`default_tool_registry` in `src/browser/mod.rs` without a CLI release).
   - **Dependency**: none of these methods exist in `dispatch()` today (only `snapshot` exists, and it's the `about:blank` stub at headless.rs:336-348). This command group is blocked on the daemon-wiring task adding `navigate`/`click`/etc. RPC methods backed by a real `TauriBrowserRuntime`/`BrowserEngine`. The CLI's method names above are chosen to be the direct RPC method names the daemon should expose, so the two tasks converge on one contract instead of the CLI inventing its own naming the daemon then has to be bent to match.

3. **Agent**
   - `neurobrowser ask "<prompt>" [--session ID] [--page N] [--json]` — maps to an `ask` RPC method mirroring `main.rs:300-361`'s `ask` Tauri command (policy-gated via `execute_with_policy`, same as the desktop app — the CLI must not get a laxer path).
   - `neurobrowser run start "<prompt>" [--session ID] [--page N]` / `neurobrowser run cancel <run-id>` — mirrors `start_agent_run` (main.rs:383) / `cancel_agent_run` (main.rs:454), returning the full `AgentRunResult` (events, status, `pending_tool_call`, `approval_id`) for scripting, whereas `ask` returns the friendlier `AskResult`-shaped summary (`response`, `tools_used`, `iterations`) `main.rs`'s own `ask` returns.
   - `neurobrowser approval submit <run-id> --approve|--deny [--message TEXT]` — mirrors `submit_approval` (main.rs:426-451); required whenever a run's `status` is `AwaitingApproval`.

4. **Policy** (mirrors `get_action_policy`/`set_action_policy`, main.rs:364-380, and the existing `policy.get`/`policy.set`/`policy.evaluate`/`policy.snapshot` RPC methods already live in headless.rs today):
   - `neurobrowser policy get`
   - `neurobrowser policy set [--file policy.json] [--autonomy read-only|assisted|high-autonomy] [--allow-domain D]... [--deny-domain D]... [--deny-tool T]... [--require-approval T]... [--block-prompt-injection true|false]` — flags map 1:1 to `ActionPolicy`'s 6 fields (policy.rs:37-44); `--file` lets a full JSON document be supplied for the less-common bulk case.
   - `neurobrowser policy evaluate --tool NAME [--arg k=v]...` — wraps the existing `policy.evaluate` method (headless.rs:322-335).
   - `neurobrowser policy snapshot` — wraps the existing `policy.snapshot` method (headless.rs:349-356).

5. **Sessions / workers** (mirrors `create_session`/`list_sessions`/`create_page`/`close_page`, main.rs:171-525, and `WorkerSpec`/`WorkerSummary`/`WorkerSnapshot`, worker.rs:24-94):
   - `neurobrowser session create` / `neurobrowser session list` / `neurobrowser session close-page --session ID --page N`
   - `neurobrowser workers list --session ID`
   - `neurobrowser workers get --session ID --worker ID`
   - `neurobrowser workers create --session ID --name NAME --goal "<text>" [--policy-file policy.json] [--max-iterations N] [--pin-page N]` — builds a `WorkerSpec`.

6. **Provider** — `neurobrowser provider set --type anthropic|openai|ollama|custom [--model NAME]` — reuses the exact env-var names `provider_config_for` (main.rs:97-135) already reads (`ANTHROPIC_API_KEY`/`ANTHROPIC_MODEL`, `OPENAI_API_KEY`/`OPENAI_MODEL`, `OLLAMA_BASE_URL`/`OLLAMA_MODEL`, `CUSTOM_PROVIDER_*`). **The CLI never accepts an API key as a bare argv flag** — argv is visible to other local users via `ps`/`/proc`; keys must come from the environment or `--api-key-file`, consistent with never putting secrets on a command line.

7. **Self-documentation** — `neurobrowser skills [get <topic>]` (from `docs/goals/feature-backlog-2026-07-20.md:145`, and modeled on `agent-browser skills get core`/`skills list`): prints `docs/AGENT-SURFACE.md`/`SKILL.md` content served from the *running daemon's* build (a small `skills.get` RPC method returning the checked-in doc text) rather than a copy baked into the CLI binary, so instructions can't go stale relative to the daemon version — the same design principle `agent-browser`'s SKILL.md states explicitly ("instructions always match the installed CLI version instead of going stale between releases").

## Output formats

- Default: human-readable — short text for scalar results (`get-text`, `ping`), a small table for list results (`session list`, `workers list`), and an indented tree for `snapshot`'s `ref_map`/`tree`.
- `--json`: emit exactly the daemon's `result` (or `error`) value as one JSON document to stdout, newline-terminated, nothing else — composes directly with `jq` (`neurobrowser snapshot --json | jq '.ref_map'`), matching the "all major commands support `--json`" convention `agent-browser` documents.
- Errors always go to stderr regardless of `--json`, so stdout stays parseable even on failure.

## Exit codes (script-friendly, not just 0/1)

- `0` — `ok:true`
- `2` — `ok:false` with `error.code == "BLOCKED"` (policy denied) — distinguishable from a hard failure so scripts can branch on "the site is disallowed" vs. "something broke"
- `3` — `AgentRunStatus::AwaitingApproval` (a run needs `approval submit`) — distinct from both success and failure since it's neither
- `4` — `error.code == "TIMEOUT"`
- `5` — transport/connection failure (daemon unreachable at the resolved socket/TCP endpoint)
- `1` — any other `ok:false`/`INTERNAL`/`UNKNOWN_METHOD`
- `64` — CLI usage error (clap's own default `sysexits.h` `EX_USAGE` behavior, left as-is rather than overridden)

## Distribution

- `cargo build --release --manifest-path cli/Cargo.toml` → `target/release/neurobrowser`, or `cargo install --path cli` for a `~/.cargo/bin/neurobrowser` install — same mechanism already used for `neurobrowser-headless` (`cargo build --release --manifest-path src-tauri/Cargo.toml --features headless`, per SKILL.md:51-52), so there's one consistent "how do I get a binary" story across both companion tools.
- `verify.sh` gains one more line (`cargo check --manifest-path cli/Cargo.toml`) alongside its existing separate `src-tauri` check, preserving the existing "each sibling crate gets its own check" pattern rather than merging into a workspace.


**New work:**
- New sibling crate cli/Cargo.toml + cli/src/main.rs — clap Parser/Subcommand tree, blocking Unix/TCP JSON-RPC client, human + --json output formatters, exit-code mapping — Rust binary crate depending on `neurobrowser` (path="..") for shared serde types + clap (derive, env) + serde_json; no new deps needed in the root lib or src-tauri crates
- `neurobrowser serve` daemon-spawning wrapper + ~/.neurobrowser/daemon.json endpoint-persistence file — std::process::Command spawn of neurobrowser-headless, parse its NEUROBROWSER_LISTENING stdout line, write resolved endpoint to a small JSON state file read by later CLI invocations
- New RPC methods on the daemon side this CLI's tool/agent/session commands depend on (navigate, click, type_text, submit_form, query_selector, evaluate, get_text, get_attribute, wait_for, extract_text, screenshot, ask, session.create/list, page.create/close, workers.list/get/create, approval.submit, run.cancel, provider.set, skills.get) — NOT implemented by this task; this is the explicit dependency on the parallel daemon-wiring task, called out so the two tasks converge on identical method names/params instead of drifting — out of scope for the cli area — listed here only as the contract the CLI is designed against

**Open questions:**
- Should `neurobrowser serve` actually exec/spawn neurobrowser-headless, or should the CLI binary itself gain a `--daemon` mode (single binary, two personalities) to avoid requiring two separate `cargo build` targets for a user who just wants `cargo install neurobrowser` to work end-to-end? The sibling-crate spawn approach keeps the daemon's own dependency footprint (tauri types, even if headless-gated) fully separate from the CLI's, but costs one extra install step.
- Does the daemon-wiring task intend session/page identifiers to be daemon-generated UUIDs (matching SessionManager::create_session's uuid_v4(), session/mod.rs) or should the CLI be allowed to pass its own session name for ergonomics (agent-browser's --session <name> semantic isolation)? This affects whether `session create` returns an opaque ID the user must capture, or whether `--session my-worker` can be used directly on first use.
- Should `neurobrowser ask`/`run start` block until AgentRunResult is final, or should there also be a streaming mode wired to the already-implemented-but-unreachable StreamingAgent/StreamEvent subsystem noted in docs/goals/feature-backlog-2026-07-20.md:171 (implemented + unit-tested in tests/streaming_agent.rs but not wired to any command surface today)? If the daemon-wiring task exposes a streaming RPC, the CLI should grow a `--follow`/NDJSON-event-stream mode to match.

### library-api
**Summary:** The crate already works as a library — src-tauri's runtime.rs and the headless daemon both consume it via `path = ".."` — but it exposes no curated embedding entry-point, no examples, no doc comments, and a real functional gap: nothing in the library owns "a browser + an agent" as one unit. SessionManager tracks ReActAgent workers but never constructs a BrowserEngine per page (PageHandle.runtime_id is just a label). A caller who wants "drive a browsing task programmatically as a Rust dependency" today must hand-assemble 5 types with zero worked examples. I propose a thin `NeuroBrowser` facade (owns BrowserEngine + ReActAgent, builder-constructed from PageConfig+AgentConfig) as the one new type, a pruned/curated pub surface (single canonical import path per symbol instead of the current root+submodule duplication), pre-1.0 semver discipline fixes (Cargo.toml version is stale relative to CHANGELOG; no license/repository fields), and reuse of the already-serde-friendly "contract" types (PageSnapshot, ActionPolicy, AgentRunResult/Event, ToolDefinition, ProviderConfig, Worker* types) as the literal shared vocabulary for the daemon wire protocol, a future MCP server (via the already-declared-but-unused `schemars` dep), and the CLI — one core, thin clients.

**Design:** ## 1. Current state (grounded in source)

**Flat, duplicated export surface.** `src/lib.rs:1-32` re-exports ~30 symbols at crate root from `agent::{memory,observability,policy,streaming,worker}`, `browser`, `providers`, `session`, `tools`. Every one of these is *also* reachable via its submodule path (confirmed: `tests/*.rs` imports mix both — e.g. `tests/agent_memory_metrics.rs:11` uses `neurobrowser::agent::metrics` while `tests/workers.rs:14` uses `neurobrowser::{ActionPolicy, AgentConfig, PageConfig, SessionManager}` from root). Two valid import paths per symbol with no documented "this one is canonical" signal is exactly the kind of API surface that makes it hard to state a semver contract — nothing marks core vocabulary (`PageSnapshot`, `ActionPolicy`, `AgentRunResult`) as more stable than incidental internals (`AgentMetrics`, `CorrelationContext` in `src/agent/observability.rs:7-147`, which is process-global-singleton internal plumbing, not embedding API).

**No embedding facade exists.** To drive one page programmatically today, a consumer must:
```rust
let browser = BrowserEngine::new(PageConfig::default());                 // src/browser/mod.rs:94-118
let provider_config = ProviderConfig { provider_type: ProviderType::Anthropic, ..Default::default() };
let provider = create_provider(&provider_config);                        // src/providers/mod.rs:329-336
let agent = ReActAgent::new(AgentConfig { max_iterations: 5, provider_config }, provider); // src/agent/mod.rs:68-87
let result = agent.execute_with_policy(prompt, &browser, &ActionPolicy::default()).await?; // src/agent/mod.rs:132-137
```
Five types, manual provider wiring, and **zero worked examples** (`find . -type d -name examples` returns nothing; the closest thing is `tests/*.rs` and the daemon's `_touch_types_to_keep_them_in_scope` stub at `src-tauri/src/bin/headless.rs:365-383`).

**`SessionManager` doesn't own a browser.** `SessionManager::create_page` (`src/session/mod.rs:72-100`) constructs a `ReActAgent` via `create_provider(&agent_config.provider_config)` and returns a `PageHandle { id, runtime_id, agent }` (`src/session/mod.rs:321-326`) — but `runtime_id` is just `format!("page-runtime-{page_id}")`, a label with **no actual `BrowserEngine` or webview attached** (`src/session/mod.rs:88-90`). Wiring that id to a real `BrowserInterface` is left entirely to the caller — Tauri's `runtime.rs` does this today by implementing `BrowserInterface` for `TauriBrowserRuntime` and passing it manually per call; the headless daemon (per this task's brief) does not yet do it at all. This is the exact gap a library facade needs to close for a Rust-embedding consumer who has no Tauri/webview and just wants `BrowserEngine`.

**Blocking HTTP inside an async fn.** `BrowserEngine` holds a `reqwest::blocking::Client` (`src/browser/mod.rs:91,96-100`) and its `async fn navigate` (`src/browser/mod.rs:153-199`) calls `self.http_client.get(url).send()` / `.text()` synchronously inside that async fn (`src/browser/mod.rs:162,173`). Embedded inside an external async binary (an axum server, a multi-worker Tokio runtime), this **blocks an executor thread** for the duration of the HTTP request — a correctness/perf landmine for any embedder who isn't already single-threaded-blocking-tolerant. This must be called out explicitly in the facade's doc comments (or fixed with `tokio::task::spawn_blocking`) before advertising `BrowserEngine`/`NeuroBrowser` as safely embeddable in arbitrary async apps.

**Shared "contract" types already exist and are already reused across surfaces.** `src-tauri/src/bin/headless.rs:29-38` imports `ActionPolicy, AutonomyLevel, RiskFlag` from `neurobrowser::agent::policy`, `PageSnapshot, RiskLevel, ToolAction, ToolRegistry, ToolRisk` from `neurobrowser::tools`, and `AgentConfig, PageConfig, ReActAgent` from `neurobrowser` root — proving the daemon is *already* a lib consumer, not a separate reimplementation. `src-tauri/src/runtime.rs:2` imports `BrowserInterface, ElementInfo, PageSnapshot` and calls `neurobrowser::browser::enrich_snapshot` — the Tauri desktop runtime is a **second `BrowserInterface` implementation** alongside `BrowserEngine`, proving the trait boundary (`src/tools/mod.rs:60-103`) is the real backend-swap seam, matching the `EngineAdapter` idea already floated in `docs/goals/feature-backlog-2026-07-20.md` (NB-18) — my facade should sit *above* that trait, generic over `&dyn BrowserInterface`, not hardcode `BrowserEngine`.

Every one of these contract types already derives `Serialize + Deserialize` (`PageSnapshot` at `src/tools/mod.rs:105`, `ActionPolicy` at `src/agent/policy.rs:36`, `AgentRunResult`/`AgentRunEvent`/`AgentRunStatus` at `src/agent/policy.rs:92-153`, `ToolDefinition`/`ToolRisk` at `src/tools/contracts.rs`, `ProviderConfig` at `src/providers/mod.rs:74`, `WorkerSpec`/`WorkerSummary`/`WorkerSnapshot`/`WorkerStatus` at `src/agent/worker.rs:23-94`) — this is the natural wire vocabulary for the daemon's JSON-RPC (already true), a future MCP tool layer, and CLI output.

**`schemars` is a declared but dead dependency.** `Cargo.toml:20` declares `schemars = { version = "0.8", features = ["derive"] }` but `grep -rn "JsonSchema\|schemars" src/` returns **zero hits** — nothing derives `JsonSchema`. This is exactly the tool an MCP wrapper needs to generate tool-call JSON Schemas from the same contract structs instead of hand-duplicating schemas — currently unused, which is a missed one-core/thin-clients opportunity.

**Semver/versioning posture is currently absent.** `Cargo.toml` has no `license`, `repository`, `keywords`, `categories`, or `readme` fields (checked directly — none present). More concretely: `Cargo.toml:3` still says `version = "0.1.0"` while `CHANGELOG.md:8` already documents a shipped `[0.1.1]` release dated 2026-07-08 — the crate manifest and changelog have drifted, so today nothing enforces that a dependent (like `src-tauri/Cargo.toml:19`'s `path = ".."` dependency, which pins no version at all) actually tracks compatible versions. No `#![warn(missing_docs)]` or `#![deny(missing_docs)]` lint exists at the crate root, and doc-comment density is effectively zero: `grep -c "^///" src/lib.rs` → 0, `src/agent/mod.rs` → 0, `src/browser/mod.rs` → 4 (only on the SSRF-guard helpers, not on public API).

## 2. Proposed public API — a curated facade + pruned root

Add a new `src/facade.rs` (or `src/lib.rs`-local) type, generic over the existing `BrowserInterface` trait so it doesn't hardcode `BrowserEngine` (keeping it reusable if a future `EngineAdapter`/headless-webview backend replaces it):

```rust
/// The single supported embedding entry point: owns one browser session and
/// the ReAct agent driving it. Construct via [`NeuroBrowser::new`] for the
/// built-in non-rendering [`BrowserEngine`], or [`NeuroBrowser::with_browser`]
/// to plug in any other [`BrowserInterface`] implementation (e.g. a Tauri
/// webview runtime, for callers who already have one).
pub struct NeuroBrowser<B: BrowserInterface = BrowserEngine> {
    browser: B,
    agent: Arc<ReActAgent>,
    policy: ActionPolicy,
}

impl NeuroBrowser<BrowserEngine> {
    pub fn new(browser_config: PageConfig, agent_config: AgentConfig) -> Self { /* mirrors SessionManager::create_page's provider wiring, src/session/mod.rs:84-86 */ }
}

impl<B: BrowserInterface> NeuroBrowser<B> {
    pub fn with_browser(browser: B, agent_config: AgentConfig) -> Self { .. }
    pub fn with_policy(self, policy: ActionPolicy) -> Self { .. }
    pub async fn navigate(&self, url: &str) -> Result<(), String> { self.browser.navigate(url).await }
    pub async fn snapshot(&self) -> Result<PageSnapshot, String> { self.browser.snapshot().await }
    /// Simple path: run to completion, return the final text answer.
    pub async fn ask(&self, prompt: &str) -> Result<String, String> { self.agent.execute(prompt, &self.browser).await }
    /// Full path: policy-gated run with events + approval/blocked/cancelled status.
    pub async fn ask_with_policy(&self, prompt: &str) -> Result<AgentRunResult, String> {
        self.agent.execute_with_policy(prompt, &self.browser, &self.policy).await
    }
    pub async fn resume_approved(&self, run_id: String, approval_id: String, tool_call: ToolCall, approved: bool) -> Result<AgentRunResult, String> { .. }
}
```

This is additive (does not remove `BrowserEngine`/`ReActAgent`/`SessionManager` — those stay for the worker/multi-tab/Tauri-integration case) and directly closes the gap identified above: nothing today bundles one browser + one agent for a plain Rust caller.

## 3. What stays public vs. gets pruned

Keep at **crate root** (the "quickstart" set, doc-linked from a new module-level `//!` on `lib.rs`): `NeuroBrowser` (new), `BrowserEngine`, `PageConfig`, `AgentConfig`, `ReActAgent`, `ActionPolicy`, `AgentRunResult`/`AgentRunEvent`/`AgentRunStatus`, `PageSnapshot`, `ProviderConfig`/`ProviderType`.

Move to **submodule-only** (still `pub`, but stop re-exporting at root to remove the dual-path ambiguity): `AgentMetrics`/`CorrelationContext`/span helpers (`agent::observability` — internal telemetry, not embedding API), `AgentMemory`/`EpisodicMemory`/`SemanticMemory`/`StateMemory` (`agent::memory` — advanced/introspection, not needed for the "drive a task" quickstart), `WorkerHandle`/`WorkerSpec`/etc. (`agent::worker` — multi-tab advanced use, already namespaced sensibly), the individual `*Info` DOM structs (`ElementInfo`, `LinkInfo`, `FormInfo`, `TableInfo`, `PriceInfo` — keep these under `tools::`, they're payload fields of `PageSnapshot`, rarely constructed standalone).

Rationale: this isn't about hiding capability, it's about signaling — one clear canonical path per symbol, and a visible split between "what you need for the 90% case" (root) and "what you need for advanced/worker/telemetry use" (submodule).

## 4. Semver / versioning posture

1. Fix the `Cargo.toml`/`CHANGELOG.md` drift immediately (bump `version` to match or supersede `0.1.1`).
2. Add `license`, `repository`, `keywords`, `categories`, `readme` to `Cargo.toml` even without a crates.io publish plan — `src-tauri` already depends on this crate across a workspace boundary via an unpinned `path = ".."` dependency (`src-tauri/Cargo.toml:19`), and any future MCP-server or CLI crate will do the same; these fields are the metadata that makes "which version is this" answerable outside `git log`.
3. Adopt explicit pre-1.0 semver discipline in `CHANGELOG.md`: bump the **minor** version on any breaking Rust API change (Cargo's own 0.x convention), patch for additive/non-breaking. Add a `## Rust library API` subsection to changelog entries, separate from Tauri-command/desktop-UI changes, since `src-tauri`, an eventual MCP server, and an eventual CLI are three separate consumers with different breakage tolerance.
4. Add `#![warn(missing_docs)]` at the crate root once the facade + contract types are documented (start with `warn`, promote to `deny` once the debt is paid down — currently 0 doc comments on `lib.rs`/`agent/mod.rs` would make `deny` fail immediately).
5. Consider `cargo public-api` (or `cargo-semver-checks`) in `verify.sh` diffing against the last tagged release's public-API text dump, so an accidental breaking change to the root/`facade`/`contracts` surface is caught before merge — this is cheap since the crate is already local-only (no crates.io registry round-trip needed, it can diff two local checkouts).

## 5. Examples + doc comments

Add `examples/` (currently absent) with three worked programs mirroring the three real usage shapes already implicit in the codebase:
- `examples/single_page_ask.rs` — `NeuroBrowser::new(...).ask("...")`, the facade quickstart.
- `examples/policy_gated_run.rs` — `ask_with_policy` + handling `AgentRunStatus::AwaitingApproval` + `resume_approved`, mirroring the flow `ReActAgent::execute_with_policy` already implements at `src/agent/mod.rs:132-350`.
- `examples/worker_fanout.rs` — `SessionManager::spawn_worker` + `list_workers`/`cross_worker_observations`, mirroring `tests/workers.rs`'s usage pattern (`src/session/mod.rs:189-318`).

Add crate-level `//!` docs to `src/lib.rs` naming the four surfaces (library, headless daemon, future MCP wrapper, future CLI) and pointing at `docs/AGENT-SURFACE.md`/`CONTEXT.md`, plus `///` doc comments on every root-level re-export (currently zero) — `cargo doc --no-deps` should be added to `verify.sh` alongside the existing fmt/clippy/test chain (`CONTEXT.md`'s Verify section currently lists `cargo check --lib`, `cargo test`, `cargo clippy --all-targets`, `cargo check --manifest-path src-tauri/Cargo.toml`, `./verify.sh` — no doc build).

## 6. Consistency with the daemon/MCP/CLI surfaces

- The daemon, an MCP wrapper, and a CLI should all serialize/deserialize the **same** contract structs this facade returns (`PageSnapshot`, `AgentRunResult`, `AgentRunEvent`, `ActionPolicy`, `PolicyDecision`, `ToolDefinition`) rather than hand-rolled DTOs — they already derive Serde, so this is zero-cost today.
- Derive `JsonSchema` (the already-declared-but-unused `schemars` dep) on exactly this contract set, so a future MCP server can generate tool-call JSON Schemas directly from the Rust types instead of maintaining parallel schema definitions — turns the currently-dead dependency into the literal glue between "Rust library API" and "MCP tool schema," satisfying the task's "one core, four thin clients" goal concretely rather than aspirationally.
- The facade's `ask_with_policy`/`resume_approved` methods are deliberately named to mirror what the daemon's `dispatch()` (`src-tauri/src/bin/headless.rs:296-355`) should eventually route to (`ask` / `approval.submit` methods), so the JSON-RPC method names and the Rust method names stay in lockstep rather than diverging into separate vocabularies.

**New work:**
- src/facade.rs — NeuroBrowser<B: BrowserInterface = BrowserEngine> facade type with new/with_browser/with_policy/navigate/snapshot/ask/ask_with_policy/resume_approved — new module + re-export at crate root; generic over the existing BrowserInterface trait so it composes with Tauri's runtime and any future EngineAdapter backend
- src/lib.rs re-export pruning — keep a curated root set (facade + BrowserEngine/PageConfig/AgentConfig/ReActAgent/ActionPolicy/AgentRunResult family/PageSnapshot/ProviderConfig); drop root re-exports of agent::memory, agent::observability, agent::worker internals so each has exactly one canonical import path
- examples/ directory — single_page_ask.rs, policy_gated_run.rs, worker_fanout.rs — three runnable programs matching the three real usage shapes already implicit in tests/*.rs
- Crate-level and root-symbol doc comments + #![warn(missing_docs)] — //! module docs on src/lib.rs naming the four surfaces; /// on every root re-export; lint added once coverage is non-zero
- Cargo.toml metadata + version fix — add license/repository/keywords/categories/readme fields; bump version to reconcile with CHANGELOG.md's already-shipped 0.1.1; document pre-1.0 semver policy (minor = breaking) in CHANGELOG.md
- JsonSchema derives on the shared contract type set — activate the already-declared schemars dependency on PageSnapshot, ActionPolicy, PolicyDecision, AgentRunResult/Event/Status, ToolDefinition/ToolRisk, ProviderConfig — feeds a future MCP tool-schema generator
- verify.sh addition — add `cargo doc --no-deps` (and optionally cargo public-api / cargo-semver-checks diffing against the last tagged release) to the existing fmt+clippy+test chain

**Open questions:**
- Should the future MCP server and CLI depend on this crate directly (in-process, sharing the facade type), or exclusively wrap the headless daemon's socket protocol (per docs/PROJECT.md's current plan: 'Full CLI wrapper over the headless daemon')? This determines whether the facade's Rust API or the daemon's JSON-RPC schema is the primary contract the other three surfaces converge on.
- Is BrowserEngine (the non-rendering scraper+reqwest engine) meant to remain the default embeddable backend long-term, or is it purely a headless-daemon fallback until a real headless-webview backend (per NB-18's EngineAdapter idea) lands — this affects whether NeuroBrowser<BrowserEngine> is the right default type parameter or whether the facade should default to no browser and force an explicit choice.
- What is the target minimum-supported-Rust-version / edition policy for the crate, given no MSRV is currently declared in Cargo.toml?
- Does the project want crates.io publication at any point (which would make the license/repository/keywords fields load-bearing beyond internal hygiene), or is `path`/git-dependency-only the permanent distribution model?

### cross-cutting
**Summary:** test

**Design:** test design

**New work:**
- test — test

**Open questions:**
- test
