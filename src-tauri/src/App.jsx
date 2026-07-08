import { useCallback, useEffect, useMemo, useRef, useState } from "react";

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

function messageText(error) {
  if (error instanceof Error) return error.message;
  return String(error);
}

function countValue(snapshot, key) {
  return snapshot?.[key] ?? 0;
}

function Message({ item }) {
  return (
    <div className={`message ${item.role}`}>
      <div>{item.text}</div>
      {item.tools?.length > 0 && <div className="tools-used">Tools: {item.tools.join(", ")}</div>}
    </div>
  );
}

function TabStrip({ tabs, currentPageId, onActivate, onClose }) {
  return (
    <div className="tabs" aria-label="Open tabs">
      {tabs.map((tab) => (
        <button
          className={`tab${tab.id === currentPageId ? " active" : ""}`}
          key={tab.id}
          onClick={() => onActivate(tab.id)}
          type="button"
        >
          <span className="tab-title">{tab.title || "New Tab"}</span>
          <span
            aria-label={`Close ${tab.title || "tab"}`}
            className="tab-close"
            onClick={(event) => {
              event.stopPropagation();
              onClose(tab.id);
            }}
            role="button"
            tabIndex={0}
          >
            x
          </span>
        </button>
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
  const args = approvalEvent?.decision?.redacted_arguments ?? approval?.pending_tool_call?.arguments ?? {};
  return (
    <div className="approval-card">
      <div className="approval-title">Approval required</div>
      <div className="approval-tool">{approval?.pending_tool_call?.name || approvalEvent?.tool}</div>
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
  provider,
  setProvider,
  policyMode,
  setPolicyMode,
  url,
  setUrl,
  onNavigate,
  onNewTab,
  compact,
}) {
  return (
    <header className={compact ? "header compact" : "header"}>
      <div className="brand">
        <span className="brand-dot" />
        <span>NeuroBrowser</span>
      </div>
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
      <form
        className="url-bar"
        onSubmit={(event) => {
          event.preventDefault();
          onNavigate();
        }}
      >
        <input
          className="url-input"
          onChange={(event) => setUrl(event.target.value)}
          placeholder="Enter a URL or domain"
          value={url}
        />
        <button className="btn" type="submit">
          Go
        </button>
      </form>
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
      text:
        lane === "appkit"
          ? "Native AppKit lane ready. Page rendering stays in WKWebView while this React surface sends browser commands."
          : "Live browser runtime initialized. Navigate to a page, then ask about what you are actually seeing.",
      tools: [],
    },
  ]);

  const appendMessage = useCallback((role, text, tools = []) => {
    const id = nextMessageId.current;
    nextMessageId.current += 1;
    setMessages((items) => [...items, { id, role, text, tools }]);
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

  const syncBrowserViewport = useCallback(async () => {
    if (!adapter.rendersPageInHost || currentPageId == null || !browserStageRef.current) return;
    await adapter.syncBrowserViewport(currentPageId, browserStageRef.current.getBoundingClientRect());
  }, [adapter, currentPageId]);

  const syncBrowserViewportForPage = useCallback(
    async (pageId) => {
      if (!adapter.rendersPageInHost || pageId == null || !browserStageRef.current) return;
      await adapter.syncBrowserViewport(pageId, browserStageRef.current.getBoundingClientRect());
    },
    [adapter]
  );

  const refreshSnapshot = useCallback(
    async ({ waitForReady = true } = {}) => {
      if (currentPageId == null || !sessionId) return null;
      if (waitForReady) {
        try {
          await adapter.waitForPageReady(currentPageId, 10000);
        } catch (error) {
          console.warn("page readiness wait failed", messageText(error));
        }
      }
      const nextSnapshot = await adapter.getPageSnapshot(sessionId, currentPageId);
      updateSnapshot(currentPageId, nextSnapshot);
      return nextSnapshot;
    },
    [adapter, currentPageId, sessionId, updateSnapshot]
  );

  const activatePage = useCallback(
    async (pageId) => {
      if (pageId == null || !sessionId) return;
      setCurrentPageId(pageId);
      await adapter.setActivePage(sessionId, pageId);
      await syncBrowserViewportForPage(pageId);
      try {
        const nextSnapshot = await adapter.getPageSnapshot(sessionId, pageId);
        updateSnapshot(pageId, nextSnapshot);
      } catch (error) {
        console.warn("snapshot refresh failed", messageText(error));
      }
    },
    [adapter, sessionId, syncBrowserViewportForPage, updateSnapshot]
  );

  const createNewTab = useCallback(async () => {
    if (!sessionId) return;
    try {
      const pageId = await adapter.createPage(sessionId);
      setTabs((items) => [...items, { id: pageId, title: `Tab ${items.length + 1}` }]);
      setCurrentPageId(pageId);
      await adapter.setActivePage(sessionId, pageId);
      await syncBrowserViewportForPage(pageId);
      setStatus("New tab ready");
    } catch (error) {
      setStatus(`New tab failed: ${messageText(error)}`);
    }
  }, [adapter, sessionId, syncBrowserViewportForPage]);

  const closeTab = useCallback(
    async (pageId) => {
      if (tabs.length <= 1 || !sessionId) return;
      try {
        await adapter.closePage(sessionId, pageId);
        const closingIndex = tabs.findIndex((tab) => tab.id === pageId);
        const nextTabs = tabs.filter((tab) => tab.id !== pageId);
        setTabs(nextTabs);
        const nextTab = nextTabs[closingIndex] || nextTabs[closingIndex - 1] || nextTabs[0];
        if (nextTab) {
          setCurrentPageId(nextTab.id);
          await adapter.setActivePage(sessionId, nextTab.id);
          await syncBrowserViewportForPage(nextTab.id);
        }
      } catch (error) {
        setStatus(`Close tab failed: ${messageText(error)}`);
      }
    },
    [adapter, sessionId, syncBrowserViewportForPage, tabs]
  );

  const navigateCurrentPage = useCallback(async () => {
    if (currentPageId == null || !sessionId) return;
    const rawUrl = url.trim();
    if (!rawUrl) return;

    setLoading("Validating destination...");
    setStatus("Validating URL...");

    try {
      const validation = await adapter.validateUrl(rawUrl);
      if (!validation.valid) throw new Error(validation.error || "Invalid URL");
      setUrl(validation.normalized_url);
      setStatus(`Navigating to ${validation.normalized_url}`);
      setLoading(`Opening ${validation.normalized_url}...`);
      await adapter.navigate(sessionId, currentPageId, validation.normalized_url);
      await refreshSnapshot();
    } catch (error) {
      setStatus(`Navigation failed: ${messageText(error)}`);
    } finally {
      setLoading(null);
    }
  }, [adapter, currentPageId, refreshSnapshot, sessionId, url]);

  const runBrowserAction = useCallback(
    async (command) => {
      if (currentPageId == null || !sessionId) return;
      try {
        await adapter.browserAction(command, sessionId, currentPageId);
        await refreshSnapshot();
      } catch (error) {
        setStatus(`Browser action failed: ${messageText(error)}`);
      }
    },
    [adapter, currentPageId, refreshSnapshot, sessionId]
  );

  const askAssistant = useCallback(async () => {
    const trimmed = prompt.trim();
    if (!trimmed || currentPageId == null || !sessionId) return;
    setPrompt("");
    appendMessage("user", trimmed);
    setThinking(true);
    setStatus("Agent is working...");
    try {
      const result =
        typeof adapter.startAgentRun === "function"
          ? await adapter.startAgentRun(sessionId, currentPageId, trimmed)
          : await adapter.ask(sessionId, currentPageId, trimmed);
      const events = result.events || [];
      setActionEvents((items) => [...items, ...events]);
      if (result.status === "awaiting_approval") {
        setPendingApproval(result);
        appendMessage("assistant", result.final_response || "Approval required before continuing.");
        setStatus("Approval required");
      } else if (result.status === "blocked") {
        appendMessage("assistant", result.final_response || "Blocked by action policy.");
        setStatus("Agent action blocked");
      } else if (result.status === "cancelled") {
        appendMessage("assistant", result.final_response || "Run cancelled.");
        setStatus("Run cancelled");
      } else {
        appendMessage("assistant", result.final_response ?? result.response ?? "", result.tools_used || []);
        setStatus("Ready");
      }
      await refreshSnapshot({ waitForReady: false });
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
        const result = await adapter.submitApproval(pendingApproval.run_id, approved, null);
        setActionEvents((items) => [...items, ...(result.events || [])]);
        setPendingApproval(null);
        appendMessage("assistant", result.final_response || (approved ? "Approved action ran." : "Approval denied."));
        await refreshSnapshot({ waitForReady: false });
        setStatus(result.status === "completed" ? "Ready" : "Run cancelled");
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
      setPolicyModeValue(nextMode);
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
      setActionPolicy(nextPolicy);
      try {
        await adapter.setActionPolicy(nextPolicy);
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
        setCurrentPageId(pageId);
        await adapter.setActivePage(nextSessionId, pageId);
        try {
          const policy = await adapter.getActionPolicy();
          if (!disposed && policy) {
            setActionPolicy(policy);
            setPolicyModeValue(policy.autonomy_level || "assisted");
          }
        } catch (error) {
          console.warn("policy load failed", messageText(error));
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
  }, [adapter]);

  useEffect(() => {
    const unsubscribe = adapter.onHostEvent((event) => {
      if (!event) return;
      if (event.type === "snapshot") {
        updateSnapshot(event.pageId ?? currentPageId, event.snapshot);
      }
      if (event.type === "status") {
        setStatus(event.message);
      }
    });
    return unsubscribe;
  }, [adapter, currentPageId, updateSnapshot]);

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
    const onKeyDown = (event) => {
      const modifier = event.metaKey || event.ctrlKey;
      if (!modifier) return;
      if (event.key.toLowerCase() === "t") {
        event.preventDefault();
        createNewTab();
      }
      if (event.key.toLowerCase() === "l") {
        event.preventDefault();
        document.querySelector(".url-input")?.focus();
      }
      if (event.key.toLowerCase() === "w" && currentPageId != null) {
        event.preventDefault();
        closeTab(currentPageId);
      }
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [closeTab, createNewTab, currentPageId]);

  const stageText = useMemo(() => {
    if (lane === "appkit") return "Real pages render in the AppKit WKWebView content pane.";
    return "Live page webview attaches here. This React surface controls layout, browser commands, and AI actions while the actual page renders in a real child webview.";
  }, [lane]);

  if (compact) {
    return (
      <div className="app-shell appkit-shell">
        <Header
          compact
          onNavigate={navigateCurrentPage}
          onNewTab={createNewTab}
          policyMode={policyMode}
          provider={provider}
          setPolicyMode={selectPolicyMode}
          setProvider={selectProvider}
          setUrl={setUrl}
          url={url}
        />
        <TabStrip currentPageId={currentPageId} onActivate={activatePage} onClose={closeTab} tabs={tabs} />
        <BrowserToolbar onAction={runBrowserAction} />
        <StatsRow snapshot={snapshot} />
        <ChatPanel
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
        <div className="loading-overlay active">
          <div className="loading-card">
            <div className="spinner" />
            <div>{loading}</div>
          </div>
        </div>
      )}
      <Header
        onNavigate={navigateCurrentPage}
        onNewTab={createNewTab}
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
