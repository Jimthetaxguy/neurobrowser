import assert from "node:assert/strict";
import { test } from "node:test";
import { applyPresentedRun, presentAgentRun } from "./agentRunPresentation.js";

test("awaiting_approval keeps the run so a follow-up approval card can show", () => {
  const result = {
    run_id: "run-2",
    status: "awaiting_approval",
    final_response: "Approval required before executing browser action",
    events: [{ type: "ApprovalRequested", tool: "navigate" }],
  };
  const presentation = presentAgentRun(result);
  assert.equal(presentation.pendingApproval, result);
  assert.equal(presentation.status, "Approval required");
  assert.equal(presentation.message, "Approval required before executing browser action");
});

test("awaiting_approval uses the shared fallback when the response is empty", () => {
  const presentation = presentAgentRun({ status: "awaiting_approval", final_response: "" });
  assert.equal(presentation.pendingApproval.status, "awaiting_approval");
  assert.equal(presentation.message, "Approval required before continuing.");
});

test("blocked and cancelled clear the card and keep their own status copy", () => {
  const blocked = presentAgentRun({ status: "blocked", final_response: "Tool call blocked by action policy" });
  assert.equal(blocked.pendingApproval, null);
  assert.equal(blocked.status, "Agent action blocked");
  assert.equal(blocked.message, "Tool call blocked by action policy");

  const cancelled = presentAgentRun({ status: "cancelled", final_response: "Approval denied" });
  assert.equal(cancelled.pendingApproval, null);
  assert.equal(cancelled.status, "Run cancelled");
  assert.equal(cancelled.message, "Approval denied");
});

test("completed and failed are terminal and clear the card", () => {
  const completed = presentAgentRun({ status: "completed", final_response: "clicked" });
  assert.equal(completed.pendingApproval, null);
  assert.equal(completed.status, "Ready");
  assert.equal(completed.message, "clicked");

  const failed = presentAgentRun({ status: "failed", final_response: "Max iterations reached" });
  assert.equal(failed.pendingApproval, null);
  assert.equal(failed.status, "Run failed; inspect page");
  assert.equal(failed.message, "Max iterations reached");

  const empty = presentAgentRun({ status: "completed", final_response: null });
  assert.equal(empty.pendingApproval, null);
  assert.equal(empty.message, "");
});

for (const [runStatus, expectedStatus] of [
  ["awaiting_approval", "Approval required"],
  ["blocked", "Agent action blocked"],
  ["cancelled", "Run cancelled"],
  ["completed", "Ready"],
  ["failed", "Run failed; inspect page"],
]) {
  for (const snapshotStatus of ["Loaded: Receipt", "Ready"]) {
    test(`${runStatus} survives snapshot status ${snapshotStatus}`, async () => {
      const result = { run_id: "next-run", status: runStatus, final_response: "Run response" };
      let status = "Agent is working...";
      let pendingApproval = { run_id: "previous-run" };
      let refreshes = 0;
      const messages = [];
      await applyPresentedRun(result, {
        appendMessage: (role, message) => messages.push({ role, message }),
        setPendingApproval: (value) => { pendingApproval = value; },
        setStatus: (value) => { status = value; },
        refreshSnapshot: async () => {
          refreshes += 1;
          await Promise.resolve();
          status = snapshotStatus;
        },
      });
      assert.equal(status, expectedStatus);
      assert.equal(pendingApproval, runStatus === "awaiting_approval" ? result : null);
      assert.equal(refreshes, 1);
      assert.deepEqual(messages, [{ role: "assistant", message: "Run response" }]);
    });
  }
}

test("a failed snapshot read preserves the completed action without another approval", async () => {
  let pendingApproval = { run_id: "previous-run" };
  let status = "Agent is working...";
  const messages = [];
  await applyPresentedRun({ status: "completed", final_response: "Form submission dispatched" }, {
    appendMessage: (role, message) => messages.push({ role, message }),
    setPendingApproval: (value) => { pendingApproval = value; },
    setStatus: (value) => { status = value; },
    refreshSnapshot: async () => { throw new Error("Browser runtime read timed out"); },
  });
  assert.equal(status, "Ready");
  assert.equal(pendingApproval, null);
  assert.deepEqual(messages, [
    { role: "assistant", message: "Form submission dispatched" },
    { role: "assistant", message: "Page refresh failed: Browser runtime read timed out" },
  ]);
});

for (const [dispatch, verification, ready, status] of [
  ["unknown", "unavailable", false, "Outcome unknown; inspect page"],
  ["not_dispatched", "not_requested", false, "Action was not dispatched"],
  ["acknowledged", "not_requested", true, "Dispatch acknowledged; outcome not verified"],
  ["acknowledged", "satisfied", true, "Requested page condition observed"],
  ["acknowledged", "unsatisfied", true, "Dispatch acknowledged; inspect page"],
]) {
  test(`receipt presentation separates ${dispatch}/${verification} from outcome`, () => {
    const result = { status: "completed", final_response: "raw JSON", events: [{ type: "ToolCallResult", receipt: { dispatch, verification, page_ready: ready, message: "Receipt evidence" } }] };
    const presentation = presentAgentRun(result);
    assert.equal(presentation.status, status);
    assert.equal(presentation.message, "Receipt evidence");
    assert.equal(presentation.pendingApproval, null);
  });
}
test("prior receipt cannot hide a later approval request in the same run", () => {
  const result = { status: "awaiting_approval", events: [{ receipt: { dispatch: "acknowledged", verification: "satisfied" } }, { type: "ApprovalRequested", tool: "submit_target" }] };
  assert.equal(presentAgentRun(result).pendingApproval, result);
});
