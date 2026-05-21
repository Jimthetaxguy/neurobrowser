import { invoke as tauriInvoke } from "@tauri-apps/api/core";

function errorMessage(error) {
  if (error instanceof Error) return error.message;
  return String(error);
}

function postToAppKit(command, payload = {}) {
  const handler = window.webkit?.messageHandlers?.neurobrowser;
  if (!handler || typeof handler.postMessage !== "function") {
    throw new Error("AppKit host bridge is unavailable. Load this page inside the native WKWebView host.");
  }
  handler.postMessage({ command, payload });
}

export function createTauriHostAdapter() {
  const invoke = (command, payload = {}) => {
    if (!window.__TAURI_INTERNALS__ && !window.__TAURI__) {
      throw new Error("Tauri IPC bridge is unavailable. Run this app through the Tauri desktop runtime.");
    }
    return tauriInvoke(command, payload);
  };

  return {
    lane: "tauri",
    rendersPageInHost: true,
    async createSession() {
      return invoke("create_session");
    },
    async createPage(sessionId) {
      return invoke("create_page", { sessionId });
    },
    async closePage(sessionId, pageId) {
      return invoke("close_page", { sessionId, pageId });
    },
    async setActivePage(sessionId, pageId) {
      return invoke("set_active_page", { sessionId, pageId });
    },
    async syncBrowserViewport(pageId, rect) {
      return invoke("sync_browser_viewport", {
        pageId,
        x: rect.left,
        y: rect.top,
        width: rect.width,
        height: rect.height,
      });
    },
    async validateUrl(url) {
      return invoke("validate_url", { url });
    },
    async navigate(sessionId, pageId, url) {
      return invoke("navigate", { sessionId, pageId, url });
    },
    async waitForPageReady(pageId, timeoutMs = 10000) {
      return invoke("wait_for_page_ready", { pageId, timeoutMs });
    },
    async getPageSnapshot(sessionId, pageId) {
      return invoke("get_page_snapshot", { sessionId, pageId });
    },
    async ask(sessionId, pageId, prompt) {
      return invoke("ask", { sessionId, pageId, prompt });
    },
    async startAgentRun(sessionId, pageId, prompt) {
      return invoke("start_agent_run", { sessionId, pageId, prompt });
    },
    async submitApproval(runId, approved, message = null) {
      return invoke("submit_approval", { runId, approved, message });
    },
    async cancelAgentRun(runId) {
      return invoke("cancel_agent_run", { runId });
    },
    async getActionPolicy() {
      return invoke("get_action_policy");
    },
    async setActionPolicy(policy) {
      return invoke("set_action_policy", { policy });
    },
    async browserAction(command, sessionId, pageId) {
      return invoke(command, { sessionId, pageId });
    },
    async setProvider(provider) {
      return invoke("set_provider", { provider });
    },
    onHostEvent() {
      return () => {};
    },
  };
}

export function createAppKitHostAdapter() {
  let nextPageId = 0;
  let latestSnapshot = null;
  const sessionId = `appkit-${crypto.randomUUID?.() ?? Date.now()}`;

  window.neurobrowserNativeDispatch = (event) => {
    if (event?.type === "snapshot") {
      latestSnapshot = event.snapshot;
    }
    window.dispatchEvent(new CustomEvent("neurobrowser:native", { detail: event }));
  };

  const send = async (command, payload = {}) => {
    postToAppKit(command, payload);
  };

  return {
    lane: "appkit",
    rendersPageInHost: false,
    async createSession() {
      await send("create_session", { sessionId });
      return sessionId;
    },
    async createPage(activeSessionId) {
      const pageId = nextPageId;
      nextPageId += 1;
      await send("create_page", { sessionId: activeSessionId, pageId });
      return pageId;
    },
    async closePage(activeSessionId, pageId) {
      await send("close_page", { sessionId: activeSessionId, pageId });
    },
    async setActivePage(activeSessionId, pageId) {
      await send("set_active_page", { sessionId: activeSessionId, pageId });
    },
    async syncBrowserViewport() {},
    async validateUrl(url) {
      const trimmed = url.trim();
      if (!trimmed) return { valid: false, normalized_url: "", error: "URL is empty" };
      if (trimmed.includes("javascript:") || trimmed.includes("data:")) {
        return { valid: false, normalized_url: trimmed, error: "Dangerous URL scheme blocked" };
      }
      const normalized =
        trimmed.startsWith("http://") || trimmed.startsWith("https://")
          ? trimmed
          : `https://${trimmed}`;
      return { valid: true, normalized_url: normalized, error: null };
    },
    async navigate(activeSessionId, pageId, url) {
      await send("navigate", { sessionId: activeSessionId, pageId, url });
    },
    async waitForPageReady() {},
    async getPageSnapshot(activeSessionId, pageId) {
      await send("get_page_snapshot", { sessionId: activeSessionId, pageId });
      return latestSnapshot;
    },
    async ask(activeSessionId, pageId, prompt) {
      await send("ask", { sessionId: activeSessionId, pageId, prompt });
      return {
        response: "Native AppKit lane received the request. Rust agent execution is the remaining bridge work.",
        tools_used: ["appkit_host_bridge"],
        iterations: 1,
      };
    },
    async startAgentRun(activeSessionId, pageId, prompt) {
      await send("start_agent_run", { sessionId: activeSessionId, pageId, prompt });
      return {
        run_id: `appkit-run-${Date.now()}`,
        status: "completed",
        final_response: "Native AppKit lane received the run request. Rust approval-loop parity remains deferred.",
        iterations: 1,
        events: [],
        pending_tool_call: null,
        approval_id: null,
      };
    },
    async submitApproval(runId, approved, message = null) {
      await send("submit_approval", { runId, approved, message });
      return {
        run_id: runId,
        status: approved ? "completed" : "cancelled",
        final_response: approved ? "Approved in native host lane." : "Approval denied.",
        iterations: 0,
        events: [],
        pending_tool_call: null,
        approval_id: null,
      };
    },
    async cancelAgentRun(runId) {
      await send("cancel_agent_run", { runId });
      return {
        run_id: runId,
        status: "cancelled",
        final_response: "Run cancelled.",
        iterations: 0,
        events: [],
        pending_tool_call: null,
        approval_id: null,
      };
    },
    async getActionPolicy() {
      return {
        autonomy_level: "assisted",
        allowed_domains: [],
        denied_domains: [],
        denied_tools: [],
        approval_required_tools: [],
        block_prompt_injection: true,
      };
    },
    async setActionPolicy(policy) {
      await send("set_action_policy", { policy });
      return policy;
    },
    async browserAction(command, activeSessionId, pageId) {
      await send(command, { sessionId: activeSessionId, pageId });
    },
    async setProvider(provider) {
      await send("set_provider", { provider });
      return { provider, model: "native-host", configured: true };
    },
    onHostEvent(callback) {
      const handler = (event) => {
        try {
          callback(event.detail);
        } catch (error) {
          console.error("native host event handler failed", errorMessage(error));
        }
      };
      window.addEventListener("neurobrowser:native", handler);
      return () => window.removeEventListener("neurobrowser:native", handler);
    },
  };
}
