import assert from "node:assert/strict";
import { test } from "node:test";
import { AGENT_TURN_IDLE, agentTurnReducer, agentTurnView } from "./agentTurn.js";

const run = (id) => ({ run_id: id, status: "awaiting_approval" });
const reduce = (turn, ...actions) => actions.reduce(agentTurnReducer, turn);

test("idle allows asking and shows no approval", () => {
  const view = agentTurnView(AGENT_TURN_IDLE);
  assert.equal(view.canAsk, true);
  assert.equal(view.approval, null);
  assert.equal(view.providerBusy, false);
  assert.equal(view.canResolveApproval, false);
});

test("a provider request runs to completion and returns to idle", () => {
  const asking = reduce(AGENT_TURN_IDLE, { type: "askStarted", id: "a" });
  assert.deepEqual(asking, { kind: "awaitingProvider", id: "a", pendingRun: null });
  assert.equal(agentTurnView(asking).canAsk, false);
  assert.equal(agentTurnView(asking).providerBusy, true);
  // A second ask is rejected while the first is in flight.
  assert.equal(agentTurnReducer(asking, { type: "askStarted", id: "b" }), asking);
  // A stale settle for another request is ignored.
  assert.equal(agentTurnReducer(asking, { type: "askSettled", id: "b" }), asking);
  assert.equal(reduce(asking, { type: "askSettled", id: "a" }), AGENT_TURN_IDLE);
});

test("approval while a provider request is in flight is held, then surfaced", () => {
  let turn = reduce(AGENT_TURN_IDLE, { type: "approvalRequested", run: run("r1") });
  assert.equal(agentTurnView(turn).canResolveApproval, true);
  assert.equal(agentTurnView(turn).canAsk, true);

  turn = agentTurnReducer(turn, { type: "askStarted", id: "a" });
  assert.equal(turn.kind, "awaitingProvider");
  const view = agentTurnView(turn);
  assert.equal(view.approval.run_id, "r1");
  assert.equal(view.canResolveApproval, false);

  // Approve is blocked: the turn does not change and the provider request keeps its id.
  assert.equal(agentTurnReducer(turn, { type: "resolveStarted", runId: "r1" }), turn);

  turn = agentTurnReducer(turn, { type: "askSettled", id: "a" });
  assert.deepEqual(turn, { kind: "awaitingApproval", run: run("r1") });
});

test("an approval that arrives during a provider request is kept", () => {
  let turn = reduce(AGENT_TURN_IDLE, { type: "askStarted", id: "a" }, { type: "approvalRequested", run: run("r2") });
  assert.equal(turn.kind, "awaitingProvider");
  assert.equal(turn.pendingRun.run_id, "r2");
  turn = agentTurnReducer(turn, { type: "askSettled", id: "a" });
  assert.equal(turn.kind, "awaitingApproval");
});

test("resolving blocks asking and duplicate resolution", () => {
  let turn = reduce(AGENT_TURN_IDLE, { type: "approvalRequested", run: run("r1") }, { type: "resolveStarted", runId: "r1" });
  assert.equal(turn.kind, "resolving");
  const view = agentTurnView(turn);
  assert.equal(view.canAsk, false);
  assert.equal(view.canResolveApproval, false);
  assert.equal(view.resolving, true);
  assert.equal(view.approval.run_id, "r1");
  assert.equal(agentTurnReducer(turn, { type: "resolveStarted", runId: "r1" }), turn);
  assert.equal(agentTurnReducer(turn, { type: "askStarted", id: "a" }), turn);
  // A settle for a different run is ignored.
  assert.equal(agentTurnReducer(turn, { type: "resolveSettled", runId: "other", next: null }), turn);
  assert.equal(agentTurnReducer(turn, { type: "resolveSettled", runId: "r1", next: null }), AGENT_TURN_IDLE);
});

test("resolution can lead to another approval or back to the same one on failure", () => {
  const resolving = reduce(AGENT_TURN_IDLE, { type: "approvalRequested", run: run("r1") }, { type: "resolveStarted", runId: "r1" });
  assert.deepEqual(
    agentTurnReducer(resolving, { type: "resolveSettled", runId: "r1", next: run("r1b") }),
    { kind: "awaitingApproval", run: run("r1b") }
  );
  assert.deepEqual(
    agentTurnReducer(resolving, { type: "resolveSettled", runId: "r1", failed: true }),
    { kind: "awaitingApproval", run: run("r1") }
  );
});

test("a newer approval that arrives while resolving is surfaced afterwards", () => {
  let turn = reduce(
    AGENT_TURN_IDLE,
    { type: "approvalRequested", run: run("r1") },
    { type: "resolveStarted", runId: "r1" },
    { type: "approvalRequested", run: run("r2") },
  );
  assert.equal(turn.kind, "resolving");
  assert.equal(turn.run.run_id, "r1");
  turn = agentTurnReducer(turn, { type: "resolveSettled", runId: "r1", next: null });
  assert.deepEqual(turn, { kind: "awaitingApproval", run: run("r2") });
});

test("a newer approval replaces the one awaiting review", () => {
  const turn = reduce(AGENT_TURN_IDLE, { type: "approvalRequested", run: run("r1") }, { type: "approvalRequested", run: run("r2") });
  assert.equal(turn.run.run_id, "r2");
});

test("unknown actions and malformed runs leave the turn unchanged", () => {
  assert.equal(agentTurnReducer(AGENT_TURN_IDLE, { type: "nope" }), AGENT_TURN_IDLE);
  assert.equal(agentTurnReducer(AGENT_TURN_IDLE, { type: "approvalRequested", run: null }), AGENT_TURN_IDLE);
  assert.equal(agentTurnReducer(AGENT_TURN_IDLE, { type: "askStarted", id: "" }), AGENT_TURN_IDLE);
});
