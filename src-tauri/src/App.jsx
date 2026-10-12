import { useCallback, useEffect, useRef, useState } from "react";
import { applyPresentedRun, presentAgentRun } from "./agentRunPresentation.js";
import { EvidencePanel } from "./EvidencePanel.jsx";
import { Omnibox } from "./OmniboxSuggestions.jsx";
import { nativePageUpdates } from "./nativePageEvents.js";
import {
  createNativeNavigationState,
  forgetNativePage,
  nativeStatusUpdate,
  notePanelNavigation,
} from "./nativeNavigationState.js";

const PROVIDERS = [
  { value: "openai", label: "OpenAI" },
  { value: "anthropic", label: "Anthropic" },
  { value: "ollama", label: "Ollama" },
  { value: "custom", label: "Custom" },
];

const POLICY_MODES = [
  { value: "assisted", label: "Assisted" },
  { value: "read_only", label: "Read-only" },
  { value: "high_autonomy", label: "High autonomy" },
];

const EMPTY_COUNTS = {
  link_count: 0,
  image_count: 0,
  form_count: 0,
  price_count: 0,
  table_count: 0,
};

const RELOAD_UNCONFIRMED_STATUS = "Reload dispatched; page readiness is unconfirmed";

const APPKIT_ACTION_STATUS = {
  browser_back: "Going back…",
  browser_forward: "Going forward…",
  browser_reload: "Reloading…",
};

function appKitSnapshotApplies(loadRef, event) {
  const load = loadRef.current;
  if (load.blocked) return false;
  if (!load.expecting) return true;
  if (event?.loadGeneration !== load.epoch) return false;
  loadRef.current = { epoch: load.epoch, expecting: false, blocked: false };
  return true;
}

const TEXT_ENTRY_TYPES = new Set(["email", "number", "password", "search", "tel", "text", "url"]);

function messageText(error) {
  if (error instanceof Error) return error.message;
  return String(error);
}

function isContentEditableElement(element) {
  if (element.isContentEditable) return true;
  for (let node = element; node && node.nodeType === 1; node = node.parentElement) {
    const value = node.getAttribute?.("contenteditable");
    if (value == null) continue;
    return value.toLowerCase() !== "false";
  }
  return false;
}

function isTextEntryTarget(target) {
  const element = target?.nodeType === 1 ? target : target?.parentElement;
  if (!element || element.nodeType !== 1) return false;
  if (isContentEditableElement(element)) return true;
  const tag = element.tagName;
  if (tag === "TEXTAREA") return true;
  if (tag !== "INPUT") return false;
  return TEXT_ENTRY_TYPES.has((element.getAttribute("type") || "text").toLowerCase());
}

function typingInTextField(event) {
  if (isTextEntryTarget(event.target)) return true;
  return isTextEntryTarget(document.activeElement);
}

function bracketSide(event) {
  const key = event.key;
  if (key === "[" || key === "{") return "left";
  if (key === "]" || key === "}") return "right";
  if (event.code === "BracketLeft") return "left";
  if (event.code === "BracketRight") return "right";
  return null;
}

function activateRelativeTab(tabs, currentPageId, delta, activatePage) {
  if (tabs.length < 2) return;
  const current = tabs.findIndex((tab) => tab.id === currentPageId);
  const index = current < 0 ? 0 : current;
  const next = (index + delta + tabs.length) % tabs.length;
  const tab = tabs[next];
  if (tab && tab.id !== currentPageId) activatePage(tab.id);
}

function countValue(snapshot, key) {
  return snapshot?.[key] ?? 0;
}

function Message({ item }) {
  return (
    <div className={`message ${item.role}`}>
      <div>{item.text}</div>
    </div>
  );
}

function TabStrip({ tabs, currentPageId, onActivate, onClose }) {
  const canClose = tabs.length > 1;
  return (
    <div className="tabs" aria-label="Open tabs">
      {tabs.map((tab) => (
        <div
          className={`tab${tab.id === currentPageId ? " active" : ""}`}
          key={tab.id}
          onClick={(event) => {
            if (event.target === event.currentTarget) onActivate(tab.id);
          }}
        >
          <button className="tab-title" onClick={() => onActivate(tab.id)} type="button">
            {tab.title || "New Tab"}
          </button>
          {canClose ? (
            <button
              aria-label={`Close ${tab.title || "tab"}`}
              className="tab-close"
              onClick={() => onClose(tab.id)}
              type="button"
            >
              x
            </button>
          ) : null}
        </div>
      ))}
    </div>
  );
}

function StatsRow({ snapshot }) {
  const stats = snapshot ?? EMPTY_COUNTS;
  return (
    <div className="page-stats">
      <div>
        Links <span className="stat-value">{countValue(stats, "link_count")}</span>
      </div>
      <div>
        Images <span className="stat-value">{countValue(stats, "image_count")}</span>
      </div>
      <div>
        Forms <span className="stat-value">{countValue(stats, "form_count")}</span>
      </div>
      <div>
        Prices <span className="stat-value">{countValue(stats, "price_count")}</span>
      </div>
      <div>
        Tables <span className="stat-value">{countValue(stats, "table_count")}</span>
      </div>
    </div>
  );
}

function BrowserToolbar({ onAction }) {
  return (
    <div className="browser-toolbar">
      <button className="btn btn-secondary" onClick={() => onAction("browser_back")} type="button">
        Back
      </button>
      <button className="btn btn-secondary" onClick={() => onAction("browser_forward")} type="button">
        Forward
      </button>
      <button className="btn btn-secondary" onClick={() => onAction("browser_reload")} type="button">
        Reload
      </button>
    </div>
  );
}

function latestApprovalEvent(run) {
  return run?.events?.find((event) => event.type === "ApprovalRequested") ?? null;
}

function eventSummary(event) {
  if (event?.receipt) {
    const receipt = event.receipt;
    if (receipt.dispatch === "unknown") return `${event.tool}: outcome unknown; inspect page`;
    if (receipt.dispatch === "not_dispatched") return `${event.tool}: no action dispatched`;
    const proof = receipt.verification === "satisfied" ? "page condition observed" : receipt.verification === "not_requested" ? "outcome not verified" : "page condition unconfirmed";
    return `${event.tool}: dispatch acknowledged; ${proof}`;
  }
  if (!event) return "";
  if (event.type === "ToolCallStarted") return `Started ${event.tool}`;
  if (event.type === "ToolCallResult") return `${event.success ? "Finished" : "Failed"} ${event.tool}`;
  if (event.type === "ToolCallBlocked") return `Blocked ${event.tool}`;
  if (event.type === "ApprovalRequested") return `Approval requested for ${event.tool}`;
  if (event.type === "ApprovalResolved") return event.approved ? "Approval granted" : "Approval denied";
  if (event.type === "RunCancelled") return "Run cancelled";
  if (event.type === "RunDone") return "Run done";
  return event.type;
}

function ApprovalCard({ approval, onResolve, onCancel }) {
  const approvalEvent = latestApprovalEvent(approval);
  const reasons = approvalEvent?.decision?.reasons ?? [];
  const args = approvalEvent?.decision?.redacted_arguments ?? {};
  const context = approval?.approval_context;
  return (
    <div className="approval-card">
      <div className="approval-title">Approval required</div>
      <div className="approval-tool">{approvalEvent?.tool}</div>
      {context && <div className="approval-context">
        <p className="evidence-source">Page: {context.url}</p>
        {context.target && <p>Target: {context.target.role} — {context.target.label || context.target.tag}</p>}
        {context.target?.destination && <p className="evidence-source">Destination: {context.target.destination}</p>}
        {context.document && <p>Reviewed revision: {context.document.revision}</p>}
      </div>}
      {reasons.length > 0 && <div className="approval-reason">{reasons.join("; ")}</div>}
      <pre className="approval-args">{JSON.stringify(args, null, 2)}</pre>
      <div className="approval-actions">
        <button className="btn" onClick={() => onResolve(true)} type="button">
          Approve
        </button>
        <button className="btn btn-secondary" onClick={() => onResolve(false)} type="button">
          Deny
        </button>
        <button className="btn btn-secondary" onClick={onCancel} type="button">
          Cancel
        </button>
      </div>
    </div>
  );
}

function ActionHistory({ events }) {
  if (!events.length) return null;
  return (
    <div className="action-history">
      <div className="action-history-title">Action history</div>
      {events.slice(-8).map((event, index) => (
        <div className={`action-event ${event.type}`} key={`${event.type}-${index}`}>
          {eventSummary(event)}
        </div>
      ))}
    </div>
  );
}

function ChatPanel({
  evidencePanel,
  actionEvents,
  messages,
  onCancelApproval,
  onResolveApproval,
  pendingApproval,
  thinking,
  prompt,
  setPrompt,
  onAsk,
}) {
  return (
    <aside className="sidebar">
      <div className="sidebar-header">AI Assistant</div>
      <div className="sidebar-evidence">{evidencePanel}</div>
      <div className="chat-messages">
        {messages.map((item) => (
          <Message item={item} key={item.id} />
        ))}
      </div>
      {pendingApproval && (
        <ApprovalCard
          approval={pendingApproval}
          onCancel={onCancelApproval}
          onResolve={onResolveApproval}
        />
      )}
      <ActionHistory events={actionEvents} />
      <div className={`loading${thinking ? " active" : ""}`}>Thinking...</div>
      <form
        className="chat-input-area"
        onSubmit={(event) => {
          event.preventDefault();
          onAsk();
        }}
      >
        <textarea
          className="chat-input"
          onChange={(event) => setPrompt(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter" && !event.shiftKey) {
              event.preventDefault();
              onAsk();
            }
          }}
          placeholder="Ask about the current page..."
          value={prompt}
        />
        <button className="btn" type="submit">
          Send
        </button>
      </form>
    </aside>
  );
}

function Header({
  adapter,
  compact,
  onNavigate,
  onNewTab,
  onStatus,
  policyMode,
  provider,
  setPolicyMode,
  setProvider,
  setUrl,
  url,
}) {
  return (
    <header className={compact ? "header compact" : "header"}>
      <div className="brand">
        <span className="brand-dot" />
        <span>NeuroBrowser</span>
      </div>
      {!compact && (
        <>
          <select
            className="provider-select"
            onChange={(event) => setProvider(event.target.value)}
            value={provider}
          >
            {PROVIDERS.map((item) => (
              <option key={item.value} value={item.value}>
                {item.label}
              </option>
            ))}
          </select>
          <select
            className="provider-select"
            onChange={(event) => setPolicyMode(event.target.value)}
            value={policyMode}
          >
            {POLICY_MODES.map((item) => (
              <option key={item.value} value={item.value}>
                {item.label}
              </option>
            ))}
          </select>
        </>
      )}
      <Omnibox
        adapter={adapter}
        onNavigate={onNavigate}
        onStatus={onStatus}
        setUrl={setUrl}
        url={url}
      />
      <button className="btn btn-secondary" onClick={onNewTab} type="button">
        New Tab
      </button>
    </header>
  );
}

export default function App({ adapter, lane }) {
  const compact = lane === "appkit";
  const browserStageRef = useRef(null);
  const nextMessageId = useRef(0);
  const [sessionId, setSessionId] = useState(null);
  const [currentPageId, setCurrentPageId] = useState(null);
  const currentPageIdRef = useRef(null);
  const setActivePageId = useCallback((pageId) => {
    currentPageIdRef.current = pageId;
    setCurrentPageId(pageId);
  }, []);
  const [tabs, setTabs] = useState([]);
  const [url, setUrl] = useState("");
  const [provider, setProviderValue] = useState("openai");
  const [policyMode, setPolicyModeValue] = useState("assisted");
  const [actionPolicy, setActionPolicy] = useState(null);
  const [actionEvents, setActionEvents] = useState([]);
  const [pendingApproval, setPendingApproval] = useState(null);
  const [snapshot, setSnapshot] = useState(null);
  const [status, setStatus] = useState("Starting session...");
  const [loading, setLoading] = useState(null);
  const [prompt, setPrompt] = useState("");
  const [thinking, setThinking] = useState(false);
  const [messages, setMessages] = useState([
    {
      id: -1,
      role: "assistant",
      text: "Live browser runtime initialized. Navigate to a page, then ask about what you are actually seeing.",
    },
  ]);
  const nativeNavigationRef = useRef(createNativeNavigationState());

  const appendMessage = useCallback((role, text) => {
    const id = nextMessageId.current;
    nextMessageId.current += 1;
    setMessages((items) => [...items, { id, role, text }]);
  }, []);

  const updateSnapshot = useCallback((pageId, nextSnapshot) => {
    if (!nextSnapshot) return;
    setSnapshot(nextSnapshot);
    setUrl(nextSnapshot.url || "");
    setTabs((items) =>
      items.map((tab) =>
        tab.id === pageId ? { ...tab, title: nextSnapshot.title || tab.title || "Untitled" } : tab
      )
    );
    setStatus(nextSnapshot.title ? `Loaded: ${nextSnapshot.title}` : "Ready");
  }, []);

  const syncBrowserViewportForPage = useCallback(
    async (pageId) => {
      if (!adapter.rendersPageInHost || pageId == null || !browserStageRef.current) return;
      await adapter.syncBrowserViewport(pageId, browserStageRef.current.getBoundingClientRect());
    },
    [adapter]
  );

  const syncBrowserViewport = useCallback(async () => {
    await syncBrowserViewportForPage(currentPageId);
  }, [currentPageId, syncBrowserViewportForPage]);

  const refreshSnapshot = useCallback(async () => {
    if (currentPageId == null || !sessionId) return null;
    const nextSnapshot = await adapter.getPageSnapshot(sessionId, currentPageId);
    updateSnapshot(currentPageId, nextSnapshot);
    return nextSnapshot;
  }, [adapter, currentPageId, sessionId, updateSnapshot]);

  // AppKit: the native host reports the outcome of this action as navigation,
  // status, and snapshot events for the page.
  const expectNativeNavigation = useCallback((pageId) => {
    if (compact) notePanelNavigation(nativeNavigationRef.current, pageId);
  }, [compact]);

  // A completed navigation or toolbar action must not look failed merely because
  // the following page read failed. Same split as applyPresentedRun.
  const performPageAction = useCallback(async (action, failureLabel) => {
    try {
      await action();
    } catch (error) {
      const message = messageText(error);
      if (message.startsWith(RELOAD_UNCONFIRMED_STATUS)) {
        setStatus(message);
        return;
      }
      setStatus(`${failureLabel}: ${message}`);
      return;
    }
    // AppKit navigate/back/forward/reload only post a message. The cached
    // snapshot is the previous page, not this load.
    if (compact) return;
    try {
      await refreshSnapshot();
    } catch (error) {
      setStatus(`Page refresh failed: ${messageText(error)}`);
    }
  }, [compact, refreshSnapshot]);

  const activatePage = useCallback(
    async (pageId) => {
      if (pageId == null || !sessionId) return;
      setActivePageId(pageId);
      await adapter.setActivePage(sessionId, pageId);
      await syncBrowserViewportForPage(pageId);
      // AppKit sends the selected tab's live state itself; a cached snapshot
      // may belong to an earlier load.
      if (compact) return;
      try {
        const nextSnapshot = await adapter.getPageSnapshot(sessionId, pageId);
        updateSnapshot(pageId, nextSnapshot);
      } catch (error) {
        setStatus(`Page refresh failed: ${messageText(error)}`);
      }
    },
    [adapter, compact, sessionId, setActivePageId, syncBrowserViewportForPage, updateSnapshot]
  );

  const createNewTab = useCallback(async () => {
    if (!sessionId) return;
    try {
      const pageId = await adapter.createPage(sessionId);
      setTabs((items) => [...items, { id: pageId, title: `Tab ${items.length + 1}` }]);
      setActivePageId(pageId);
      await syncBrowserViewportForPage(pageId);
      setStatus("New tab ready");
    } catch (error) {
      setStatus(`New tab failed: ${messageText(error)}`);
    }
  }, [adapter, sessionId, setActivePageId, syncBrowserViewportForPage]);

  const closeTab = useCallback(
    async (pageId) => {
      if (tabs.length <= 1 || !sessionId) return;
      try {
        await adapter.closePage(sessionId, pageId);
        forgetNativePage(nativeNavigationRef.current, pageId);
        const closingIndex = tabs.findIndex((tab) => tab.id === pageId);
        const nextTabs = tabs.filter((tab) => tab.id !== pageId);
        setTabs(nextTabs);
        const nextTab = nextTabs[closingIndex] || nextTabs[closingIndex - 1] || nextTabs[0];
        if (nextTab) {
          setActivePageId(nextTab.id);
          await adapter.setActivePage(sessionId, nextTab.id);
          await syncBrowserViewportForPage(nextTab.id);
          if (!compact) {
            try {
              const nextSnapshot = await adapter.getPageSnapshot(sessionId, nextTab.id);
              updateSnapshot(nextTab.id, nextSnapshot);
            } catch (error) {
              setStatus(`Page refresh failed: ${messageText(error)}`);
            }
          }
        }
      } catch (error) {
        setStatus(`Close tab failed: ${messageText(error)}`);
      }
    },
    [adapter, compact, sessionId, setActivePageId, syncBrowserViewportForPage, tabs, updateSnapshot]
  );

  const navigateCurrentPage = useCallback(async (nextUrl) => {
    if (currentPageId == null || !sessionId) return;
    const rawUrl = (typeof nextUrl === "string" ? nextUrl : url).trim();
    if (!rawUrl) return;

    setStatus(`Navigating to ${rawUrl}`);
    setLoading(`Opening ${rawUrl}...`);
    expectNativeNavigation(currentPageId);

    try {
      await performPageAction(
        () => adapter.navigate(sessionId, currentPageId, rawUrl),
        "Navigation failed"
      );
    } finally {
      setLoading(null);
    }
  }, [adapter, currentPageId, expectNativeNavigation, performPageAction, sessionId, url]);

  const runBrowserAction = useCallback(
    async (command) => {
      if (currentPageId == null || !sessionId) return;
      if (compact) setStatus(APPKIT_ACTION_STATUS[command] || "Working…");
      expectNativeNavigation(currentPageId);
      await performPageAction(
        () => adapter.browserAction(command, sessionId, currentPageId),
        "Browser action failed"
      );
    },
    [adapter, compact, currentPageId, expectNativeNavigation, performPageAction, sessionId]
  );

  const askAssistant = useCallback(async () => {
    const trimmed = prompt.trim();
    if (!trimmed || currentPageId == null || !sessionId) return;
    setPrompt("");
    appendMessage("user", trimmed);
    setThinking(true);
    setStatus("Agent is working...");
    try {
      const result = await adapter.startAgentRun(sessionId, currentPageId, trimmed);
      const events = result.events || [];
      setActionEvents((items) => [...items, ...events]);
      await applyPresentedRun(result, { appendMessage, setPendingApproval, setStatus, refreshSnapshot });
    } catch (error) {
      appendMessage("assistant", `Request failed: ${messageText(error)}`);
      setStatus("Agent request failed");
    } finally {
      setThinking(false);
    }
  }, [adapter, appendMessage, currentPageId, prompt, refreshSnapshot, sessionId]);

  const resolveApproval = useCallback(
    async (approved) => {
      if (!pendingApproval) return;
      setThinking(true);
      try {
        const result = await adapter.submitApproval(pendingApproval.run_id, approved);
        setActionEvents((items) => [...items, ...(result.events || [])]);
        await applyPresentedRun(result, { appendMessage, setPendingApproval, setStatus, refreshSnapshot });
      } catch (error) {
        appendMessage("assistant", `Approval failed: ${messageText(error)}`);
        setStatus("Approval failed");
      } finally {
        setThinking(false);
      }
    },
    [adapter, appendMessage, pendingApproval, refreshSnapshot]
  );

  const cancelApproval = useCallback(async () => {
    if (!pendingApproval) return;
    try {
      const result = await adapter.cancelAgentRun(pendingApproval.run_id);
      setActionEvents((items) => [...items, ...(result.events || [])]);
      setPendingApproval(null);
      appendMessage("assistant", result.final_response || "Run cancelled.");
      setStatus("Run cancelled");
    } catch (error) {
      setStatus(`Cancel failed: ${messageText(error)}`);
    }
  }, [adapter, appendMessage, pendingApproval]);

  const presentProposal = useCallback(async (result, source) => {
    setActionEvents((items) => [...items, ...(result.events || []).map(event => ({ ...event, source }))]);
    if (source && source.pageId !== currentPageIdRef.current) {
      const presentation = presentAgentRun(result);
      appendMessage("assistant", `Action on ${source.pageUrl || "previous page"}: ${presentation.message}`);
      if (result.status === "awaiting_approval") setPendingApproval(result);
      setStatus(presentation.status);
      return;
    }
    await applyPresentedRun(result, { appendMessage, setPendingApproval, setStatus, refreshSnapshot });
  }, [appendMessage, refreshSnapshot]);

  const selectProvider = useCallback(
    async (nextProvider) => {
      setProviderValue(nextProvider);
      try {
        const result = await adapter.setProvider(nextProvider);
        const configuredText = result.configured ? "configured" : "missing credentials";
        setStatus(`Provider: ${result.provider} (${result.model}, ${configuredText})`);
      } catch (error) {
        setStatus(`Provider change failed: ${messageText(error)}`);
      }
    },
    [adapter]
  );

  const selectPolicyMode = useCallback(
    async (nextMode) => {
      setStatus("Applying policy after the current governed operation…");
      const nextPolicy = {
        ...(actionPolicy || {
          allowed_domains: [],
          denied_domains: [],
          denied_tools: [],
          approval_required_tools: [],
          block_prompt_injection: true,
        }),
        autonomy_level: nextMode,
      };
      try {
        const accepted = await adapter.setActionPolicy(nextPolicy);
        setActionPolicy(accepted || nextPolicy);
        setPolicyModeValue(nextMode);
        setStatus(`Policy: ${nextMode.replace("_", " ")}`);
      } catch (error) {
        setStatus(`Policy update failed: ${messageText(error)}`);
      }
    },
    [actionPolicy, adapter]
  );

  useEffect(() => {
    let disposed = false;
    async function init() {
      try {
        const nextSessionId = await adapter.createSession();
        if (disposed) return;
        setSessionId(nextSessionId);
        const pageId = await adapter.createPage(nextSessionId);
        if (disposed) return;
        setTabs([{ id: pageId, title: "Tab 1" }]);
        setActivePageId(pageId);
        if (!compact) {
          try {
            const policy = await adapter.getActionPolicy();
            if (!disposed && policy) {
              setActionPolicy(policy);
              setPolicyModeValue(policy.autonomy_level || "assisted");
            }
          } catch (error) {
            console.warn("policy load failed", messageText(error));
          }
        }
        setStatus("Session ready");
      } catch (error) {
        setStatus(`Startup failed: ${messageText(error)}`);
      }
    }
    init();
    return () => {
      disposed = true;
    };
  }, [adapter, compact, setActivePageId]);

  useEffect(() => {
    if (typeof adapter.onHostEvent !== "function") return undefined;
    const unsubscribe = adapter.onHostEvent((event) => {
      if (!event) return;
      if (compact) {
        const updates = nativePageUpdates(event, currentPageIdRef.current);
        if (updates.tabs) {
          setTabs(updates.tabs);
          setActivePageId(updates.activePageId);
          if ("url" in updates) setUrl(updates.url);
        }
        // Status follows the active tab's newest WebKit navigation only.
        const next = nativeStatusUpdate(nativeNavigationRef.current, event, currentPageIdRef.current);
        if ("url" in next) setUrl(next.url);
        if (next.snapshot) updateSnapshot(event.pageId, next.snapshot);
        else if (next.status) setStatus(next.status);
      } else if (event.type === "status") {
        setStatus(event.message);
      }
    });
    return unsubscribe;
  }, [adapter, compact, setActivePageId, updateSnapshot]);

  useEffect(() => {
    if (!adapter.rendersPageInHost) return undefined;
    let frame = null;
    const queueSync = () => {
      if (frame) return;
      frame = window.requestAnimationFrame(async () => {
        frame = null;
        try {
          await syncBrowserViewport();
        } catch (error) {
          console.warn("viewport sync failed", messageText(error));
        }
      });
    };
    window.addEventListener("resize", queueSync);
    const observer = new ResizeObserver(queueSync);
    if (browserStageRef.current) observer.observe(browserStageRef.current);
    queueSync();
    return () => {
      window.removeEventListener("resize", queueSync);
      observer.disconnect();
      if (frame) window.cancelAnimationFrame(frame);
    };
  }, [adapter.rendersPageInHost, syncBrowserViewport]);

  useEffect(() => {
    // The page webview does not deliver keys here. These chords run only while
    // this React chrome is focused; page focus will use native menu accelerators.
    const onKeyDown = (event) => {
      const modifier = event.metaKey || event.ctrlKey;
      if (!modifier) return;
      const key = event.key.toLowerCase();
      if (key === "t") {
        if (compact) return;
        event.preventDefault();
        createNewTab();
        return;
      }
      if (key === "l") {
        event.preventDefault();
        document.querySelector(".url-input")?.focus();
        return;
      }
      if (key === "w" && currentPageId != null) {
        if (compact) return;
        event.preventDefault();
        closeTab(currentPageId);
        return;
      }

      if (compact || event.altKey) return;

      if (key === "r" && !event.shiftKey && !event.repeat) {
        event.preventDefault();
        runBrowserAction("browser_reload");
        return;
      }
      if (typingInTextField(event)) return;

      const side = bracketSide(event);
      if (side) {
        event.preventDefault();
        const shifted = event.shiftKey || event.key === "{" || event.key === "}";
        if (shifted) {
          activateRelativeTab(tabs, currentPageId, side === "left" ? -1 : 1, activatePage);
        } else if (!event.repeat) {
          runBrowserAction(side === "left" ? "browser_back" : "browser_forward");
        }
        return;
      }

      if (event.shiftKey || event.repeat || !/^[1-9]$/.test(event.key)) return;
      event.preventDefault();
      const index = event.key === "9" ? tabs.length - 1 : Number(event.key) - 1;
      if (index < 0 || index >= tabs.length) return;
      const tab = tabs[index];
      if (tab.id !== currentPageId) activatePage(tab.id);
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [activatePage, closeTab, compact, createNewTab, currentPageId, runBrowserAction, tabs]);

  const stageText =
    "Live page webview attaches here. This React surface controls layout, browser commands, and AI actions while the actual page renders in a real child webview.";

  if (compact) {
    return (
      <div className="app-shell appkit-shell">
        <Header
          adapter={adapter}
          compact
          onNavigate={navigateCurrentPage}
          onNewTab={createNewTab}
          onStatus={setStatus}
          setUrl={setUrl}
          url={url}
        />
        <TabStrip currentPageId={currentPageId} onActivate={activatePage} onClose={closeTab} tabs={tabs} />
        <BrowserToolbar onAction={runBrowserAction} />
        <StatsRow snapshot={snapshot} />
        <div className="status-bar">
          <span>{status}</span>
          <span>Tabs: {tabs.length}</span>
        </div>
      </div>
    );
  }

  return (
    <div className="app-shell">
      {loading && (
        <div className="loading-overlay">
          <div className="loading-card">
            <div className="spinner" />
            <div>{loading}</div>
          </div>
        </div>
      )}
      <Header
        adapter={adapter}
        onNavigate={navigateCurrentPage}
        onNewTab={createNewTab}
        onStatus={setStatus}
        policyMode={policyMode}
        provider={provider}
        setPolicyMode={selectPolicyMode}
        setProvider={selectProvider}
        setUrl={setUrl}
        url={url}
      />
      <TabStrip currentPageId={currentPageId} onActivate={activatePage} onClose={closeTab} tabs={tabs} />
      <main>
        <section className="browser-panel">
          <BrowserToolbar onAction={runBrowserAction} />
          <StatsRow snapshot={snapshot} />
          <div className="browser-stage" ref={browserStageRef}>
            <div className="stage-placeholder">
              <h2>{snapshot?.title || "Native page rendering"}</h2>
              <p>{stageText}</p>
            </div>
          </div>
        </section>
        <ChatPanel
          evidencePanel={<EvidencePanel adapter={adapter} sessionId={sessionId} pageId={currentPageId} pageUrl={snapshot?.url} onRun={presentProposal} />}
          actionEvents={actionEvents}
          messages={messages}
          onCancelApproval={cancelApproval}
          onAsk={askAssistant}
          onResolveApproval={resolveApproval}
          pendingApproval={pendingApproval}
          prompt={prompt}
          setPrompt={setPrompt}
          thinking={thinking}
        />
      </main>
      <div className="status-bar">
        <span>{status}</span>
        <span>Tabs: {tabs.length}</span>
      </div>
    </div>
  );
}
