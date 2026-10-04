// Status copy and approval-card lifetime shared by startAgentRun and submitApproval.
// The card stays only while the run is awaiting another approval.
export async function applyPresentedRun(result, { appendMessage, setPendingApproval, setStatus, refreshSnapshot }) {
  const presentation = presentAgentRun(result);
  setPendingApproval(presentation.pendingApproval);
  appendMessage("assistant", presentation.message);
  try {
    await refreshSnapshot();
  } catch (error) {
    // A completed action must not look like a failed approval merely because
    // its subsequent page read failed: that could invite another submission.
    const message = error instanceof Error ? error.message : String(error);
    appendMessage("assistant", `Page refresh failed: ${message}`);
  }
  setStatus(presentation.status);
}

export function presentAgentRun(result) {
  const receipt = [...(result.events || [])].reverse().find(event => event.receipt)?.receipt;
  if (receipt && ["completed", "failed"].includes(result.status)) {
    return {
      pendingApproval: null,
      message: receipt.message,
      status: receipt.dispatch === "unknown" ? "Outcome unknown; inspect page"
        : receipt.dispatch === "not_dispatched" ? "Action was not dispatched"
        : receipt.verification === "satisfied" ? "Requested page condition observed"
        : receipt.verification === "not_requested" && receipt.page_ready ? "Dispatch acknowledged; outcome not verified"
        : "Dispatch acknowledged; inspect page",
    };
  }
  if (result.status === "failed") {
    return { pendingApproval: null, message: result.final_response || "Run failed.", status: "Run failed; inspect page" };
  }
  if (result.status === "awaiting_approval") {
    return {
      pendingApproval: result,
      message: result.final_response || "Approval required before continuing.",
      status: "Approval required",
    };
  }
  if (result.status === "blocked") {
    return {
      pendingApproval: null,
      message: result.final_response || "Blocked by action policy.",
      status: "Agent action blocked",
    };
  }
  if (result.status === "cancelled") {
    return {
      pendingApproval: null,
      message: result.final_response || "Run cancelled.",
      status: "Run cancelled",
    };
  }
  return {
    pendingApproval: null,
    message: result.final_response ?? "",
    status: "Ready",
  };
}
