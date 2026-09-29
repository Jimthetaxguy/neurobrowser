import assert from "node:assert/strict";
import { test } from "node:test";
import { presentAgentRun } from "./agentRunPresentation.js";

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
  assert.equal(failed.status, "Ready");
  assert.equal(failed.message, "Max iterations reached");

  const empty = presentAgentRun({ status: "completed", final_response: null });
  assert.equal(empty.pendingApproval, null);
  assert.equal(empty.message, "");
});
