// Status copy and approval-card lifetime shared by startAgentRun and submitApproval.
// The card stays only while the run is awaiting another approval.
export function presentAgentRun(result) {
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
