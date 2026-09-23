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
    async navigate(sessionId, pageId, url) {
      return invoke("navigate", { sessionId, pageId, url });
    },
    async getPageSnapshot(sessionId, pageId) {
      return invoke("get_page_snapshot", { sessionId, pageId });
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
    async navigate(activeSessionId, pageId, url) {
      await send("navigate", { sessionId: activeSessionId, pageId, url });
    },
    async getPageSnapshot(activeSessionId, pageId) {
      await send("get_page_snapshot", { sessionId: activeSessionId, pageId });
      return latestSnapshot;
    },
    async browserAction(command, activeSessionId, pageId) {
      await send(command, { sessionId: activeSessionId, pageId });
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
