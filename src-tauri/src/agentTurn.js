// The assistant turn as one explicit state. Provider requests (reply-only) and
// governed approvals share the sidebar, so their overlap is modelled here
// instead of in separate booleans.
//
//   idle                               nothing in flight
//   awaitingProvider { id, pendingRun } a provider request is in flight; an
//                                      approval that is waiting is held in
//                                      `pendingRun` and cannot be resolved yet
//   awaitingApproval { run }           a run waits for Approve/Deny/Cancel
//   resolving { run, queuedRun }       an Approve/Deny/Cancel is in flight;
//                                      a newer approval waits in `queuedRun`
//
// Transitions that do not apply return the same object, so callers can test
// `next === turn` to learn that an action was refused.

export const AGENT_TURN_IDLE = Object.freeze({ kind: "idle" });

function awaiting(run) {
  return { kind: "awaitingApproval", run };
}

function heldRun(turn) {
  if (turn.kind === "awaitingApproval" || turn.kind === "resolving") return turn.run;
  if (turn.kind === "awaitingProvider") return turn.pendingRun;
  return null;
}

function validRun(run) {
  return run && typeof run === "object" && run.run_id != null;
}

export function agentTurnReducer(turn, action) {
  switch (action?.type) {
    case "askStarted": {
      if (typeof action.id !== "string" || !action.id) return turn;
      if (turn.kind === "idle") return { kind: "awaitingProvider", id: action.id, pendingRun: null };
      if (turn.kind === "awaitingApproval") return { kind: "awaitingProvider", id: action.id, pendingRun: turn.run };
      return turn;
    }
    case "askSettled": {
      if (turn.kind !== "awaitingProvider" || turn.id !== action.id) return turn;
      return turn.pendingRun ? awaiting(turn.pendingRun) : AGENT_TURN_IDLE;
    }
    case "approvalRequested": {
      // The newest run wins, as the approval card shows one run at a time.
      if (!validRun(action.run)) return turn;
      if (turn.kind === "awaitingProvider") return { ...turn, pendingRun: action.run };
      if (turn.kind === "resolving") return { ...turn, queuedRun: action.run };
      return awaiting(action.run);
    }
    case "resolveStarted": {
      if (turn.kind !== "awaitingApproval" || turn.run.run_id !== action.runId) return turn;
      return { kind: "resolving", run: turn.run, queuedRun: null };
    }
    case "resolveSettled": {
      if (turn.kind !== "resolving" || turn.run.run_id !== action.runId) return turn;
      if (validRun(action.next)) return awaiting(action.next);
      if (turn.queuedRun) return awaiting(turn.queuedRun);
      // A failed submission leaves the same run reviewable.
      if (action.failed) return awaiting(turn.run);
      return AGENT_TURN_IDLE;
    }
    default:
      return turn;
  }
}

/** What the sidebar may show and enable for a turn. */
export function agentTurnView(turn) {
  return {
    approval: heldRun(turn),
    canAsk: turn.kind === "idle" || turn.kind === "awaitingApproval",
    canResolveApproval: turn.kind === "awaitingApproval",
    providerBusy: turn.kind === "awaitingProvider",
    resolving: turn.kind === "resolving",
  };
}
