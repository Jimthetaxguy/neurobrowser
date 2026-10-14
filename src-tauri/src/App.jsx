import { useCallback, useEffect, useRef, useState } from "react";
import { applyPresentedRun, presentAgentRun } from "./agentRunPresentation.js";
import { EvidencePanel } from "./EvidencePanel.jsx";
import { applyDownloadUpdate, downloadProgressText } from "./downloads.js";
import { isBlankDocument } from "./historySuggestions.js";
import { NewTabPage } from "./NewTabPage.jsx";
import { Omnibox } from "./OmniboxSuggestions.jsx";
import { classifyOmniboxInput } from "./omniboxInput.js";
import { createMenuEchoGuard, numberedTabCommand } from "./menuCommands.js";
import { FindBar, chromeShortcut } from "./pageFind.js";
import {
  blockedPopupLabel,
  focusAfterClose,
  forgetBlockedPopups,
  insertOpenedTab,
  moveTab,
  nativePageUpdates,
  pageLifecycleUpdates,
  pageNewWindowRequest,
  rememberBlockedPopup,
  securityState,
  takeBlockedPopup,
} from "./nativePageEvents.js";
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

function activateNumberedTab(tabs, currentPageId, number, activatePage) {
  const index = number === 9 ? tabs.length - 1 : number - 1;
  if (index < 0 || index >= tabs.length) return;
  const tab = tabs[index];
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

function PopupBlockedIndicator({ count, onDismiss, onOpen }) {
  const label = blockedPopupLabel(count);
  if (!label) return null;
  return (
    <span className="popup-blocked" role="status">
      <span>{label}</span>
      <button onClick={onOpen} type="button">Open</button>
      <button aria-label="Dismiss blocked popup" onClick={onDismiss} type="button">×</button>
    </span>
  );
}

function SecurityIndicator({ url }) {
  const state = securityState(url);
  if (state === "secure") {
    return (
      <span aria-label="Secure" className="security-indicator secure" data-security="secure" title="Secure">
        <svg aria-hidden="true" className="security-lock" viewBox="0 0 16 16">
          <path
            d="M5 7V5a3 3 0 0 1 6 0v2h1.2A1.8 1.8 0 0 1 14 8.8v4.4a1.8 1.8 0 0 1-1.8 1.8H3.8A1.8 1.8 0 0 1 2 13.2V8.8A1.8 1.8 0 0 1 3.8 7H5zm1.2 0h3.6V5a1.8 1.8 0 0 0-3.6 0v2z"
            fill="currentColor"
          />
        </svg>
      </span>
    );
  }
  if (state === "insecure") {
    return (
      <span className="security-indicator insecure" data-security="insecure" title="Not secure">
        Not secure
      </span>
    );
  }
  return null;
}

function LoadProgress({ active }) {
  if (!active) return null;
  return <div aria-label="Loading page" className="load-progress" role="progressbar" />;
}

function Downloads({ items, onOpen, onShow }) {
  if (!items.length) return null;
  return (
    <section aria-label="Downloads" className="downloads">
      <div className="downloads-title">Downloads</div>
      <ul>
        {items.map((item) => {
          const progress = item.phase === "progress" ? downloadProgressText(item) : "";
          const percent = progress.endsWith("%") ? Number(progress.slice(0, -1)) : null;
          return (
            <li data-phase={item.phase} key={item.id}>
              <span className="download-name" title={item.url}>{item.filename}</span>
              {item.phase === "progress" ? (
                <span
                  aria-label={`Downloading ${item.filename}`}
                  aria-valuemax={percent == null ? undefined : 100}
                  aria-valuemin={percent == null ? undefined : 0}
                  aria-valuenow={percent == null ? undefined : percent}
                  className={`download-progress${percent == null ? " indeterminate" : ""}`}
                  role="progressbar"
                >
                  {progress}
                </span>
              ) : null}
              {item.phase === "done" ? (
                <>
                  <span className="download-state">Done</span>
                  <button onClick={() => onOpen(item.id)} type="button">Open</button>
                  <button onClick={() => onShow(item.id)} type="button">Show in folder</button>
                </>
              ) : null}
              {item.phase === "failed" ? (
                <span className="download-state">Failed{item.error ? `: ${item.error}` : ""}</span>
              ) : null}
            </li>
          );
        })}
      </ul>
    </section>
  );
}

function TabStrip({ tabs, currentPageId, onActivate, onClose, onReorder }) {
  const canClose = tabs.length > 1;
  const dragFrom = useRef(null);
  const suppressClick = useRef(false);
  const [draggingId, setDraggingId] = useState(null);
  const [dropIndex, setDropIndex] = useState(null);

  function clearDrag() {
    dragFrom.current = null;
    setDraggingId(null);
    setDropIndex(null);
  }

  return (
    <div className="tabs" aria-label="Open tabs">
      {tabs.map((tab, index) => (
        <div
          className={`tab${tab.id === currentPageId ? " active" : ""}${tab.id === draggingId ? " dragging" : ""}${dropIndex === index ? " drop-target" : ""}`}
          draggable
          key={tab.id}
          onClick={(event) => {
            if (suppressClick.current) {
              suppressClick.current = false;
              return;
            }
            if (event.target === event.currentTarget) onActivate(tab.id);
          }}
          onDragEnd={() => {
            if (dragFrom.current == null) return;
            suppressClick.current = true;
            clearDrag();
          }}
          onDragOver={(event) => {
            event.preventDefault();
            if (event.dataTransfer) event.dataTransfer.dropEffect = "move";
            setDropIndex(index);
          }}
          onDragStart={(event) => {
            if (event.target.closest?.(".tab-close")) {
              event.preventDefault();
              return;
            }
            dragFrom.current = index;
            suppressClick.current = false;
            setDraggingId(tab.id);
            event.dataTransfer?.setData("text/plain", String(index));
            if (event.dataTransfer) event.dataTransfer.effectAllowed = "move";
          }}
          onDrop={(event) => {
            event.preventDefault();
            const raw = event.dataTransfer?.getData("text/plain");
            const fromData = raw == null || raw === "" ? Number.NaN : Number(raw);
            const fromIndex = Number.isInteger(dragFrom.current) ? dragFrom.current : fromData;
            clearDrag();
            suppressClick.current = true;
            if (!Number.isInteger(fromIndex)) return;
            onReorder(fromIndex, index);
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

function BrowserToolbar({ loading, onAction, onStop, zoomPercent }) {
  return (
    <div className="browser-toolbar">
      <button className="btn btn-secondary" onClick={() => onAction("browser_back")} type="button">
        Back
      </button>
      <button className="btn btn-secondary" onClick={() => onAction("browser_forward")} type="button">
        Forward
      </button>
      {loading ? (
        <button className="btn btn-secondary" onClick={onStop} type="button">
          Stop
        </button>
      ) : (
        <button className="btn btn-secondary" onClick={() => onAction("browser_reload")} type="button">
          Reload
        </button>
      )}
      {Number.isFinite(zoomPercent) ? (
        <span aria-label={`Page zoom ${zoomPercent}%`} className="zoom-level">{zoomPercent}%</span>
      ) : null}
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
  blockedCount,
  compact,
  onDismissBlocked,
  onNavigate,
  onNewTab,
  onOpenBlocked,
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
      <div className="location">
        <SecurityIndicator url={url} />
        <Omnibox
          adapter={adapter}
          onNavigate={onNavigate}
          onStatus={onStatus}
          setUrl={setUrl}
          url={url}
        />
        <PopupBlockedIndicator count={blockedCount} onDismiss={onDismissBlocked} onOpen={onOpenBlocked} />
      </div>
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
  const tabsRef = useRef(tabs);
  tabsRef.current = tabs;
  const [url, setUrl] = useState("");
  const addressDirtyRef = useRef(false);
  const commandLoadToken = useRef(0);
  const setUrlFromUser = useCallback((next) => {
    addressDirtyRef.current = true;
    setUrl(next);
  }, []);
  const setUrlFromPage = useCallback((next) => {
    addressDirtyRef.current = false;
    setUrl(next);
  }, []);
  const [provider, setProviderValue] = useState("openai");
  const [policyMode, setPolicyModeValue] = useState("assisted");
  const [actionPolicy, setActionPolicy] = useState(null);
  const [actionEvents, setActionEvents] = useState([]);
  const [pendingApproval, setPendingApproval] = useState(null);
  const [snapshot, setSnapshot] = useState(null);
  const [status, setStatus] = useState("Starting session...");
  const [loading, setLoading] = useState(null);
  const [pageLoading, setPageLoading] = useState(false);
  const [blockedPopups, setBlockedPopups] = useState({});
  const [downloads, setDownloads] = useState([]);
  const [findOpen, setFindOpen] = useState(false);
  const [findQuery, setFindQuery] = useState("");
  const [findHit, setFindHit] = useState({ total: 0, current: 0 });
  const [zoomPercent, setZoomPercent] = useState(100);
  const findInputRef = useRef(null);
  const findOpenRef = useRef(false);
  const findQueryRef = useRef("");
  const findToken = useRef(0);
  findOpenRef.current = findOpen;
  findQueryRef.current = findQuery;
  const blockedPopupsRef = useRef({});
  const sessionIdRef = useRef(null);
  sessionIdRef.current = sessionId;
  const popupQueue = useRef(Promise.resolve());
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
  const menuEcho = useRef(createMenuEchoGuard());

  const appendMessage = useCallback((role, text) => {
    const id = nextMessageId.current;
    nextMessageId.current += 1;
    setMessages((items) => [...items, { id, role, text }]);
  }, []);

  const updateSnapshot = useCallback((pageId, nextSnapshot) => {
    if (!nextSnapshot) return;
    setSnapshot(nextSnapshot);
    setUrlFromPage(nextSnapshot.url || "");
    setTabs((items) =>
      items.map((tab) =>
        tab.id === pageId
          ? {
              ...tab,
              title: nextSnapshot.title || tab.title || "Untitled",
              url: typeof nextSnapshot.url === "string" ? nextSnapshot.url : tab.url,
            }
          : tab
      )
    );
    setStatus(nextSnapshot.title ? `Loaded: ${nextSnapshot.title}` : "Ready");
  }, [setUrlFromPage]);

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
      const known = tabsRef.current.find((tab) => tab.id === pageId);
      if (known && typeof known.url === "string") setUrlFromPage(known.url);
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
      refreshZoomRef.current(pageId);
    },
    [adapter, compact, sessionId, setActivePageId, setUrlFromPage, syncBrowserViewportForPage, updateSnapshot]
  );

  const createNewTab = useCallback(async () => {
    if (!sessionId) return;
    try {
      const pageId = await adapter.createPage(sessionId);
      setTabs((items) => [...items, { id: pageId, title: `Tab ${items.length + 1}`, url: "" }]);
      setActivePageId(pageId);
      setUrlFromPage("");
      await syncBrowserViewportForPage(pageId);
      setStatus("New tab ready");
      if (!compact) document.querySelector(".location .url-input")?.focus();
    } catch (error) {
      setStatus(`New tab failed: ${messageText(error)}`);
    }
  }, [adapter, compact, sessionId, setActivePageId, setUrlFromPage, syncBrowserViewportForPage]);

  const closeTab = useCallback(
    async (pageId) => {
      if (tabs.length <= 1 || !sessionId) return;
      try {
        await adapter.closePage(sessionId, pageId);
        forgetNativePage(nativeNavigationRef.current, pageId);
        const nextId = focusAfterClose(tabs, pageId, currentPageId);
        const nextTabs = tabs.filter((tab) => tab.id !== pageId);
        setTabs(nextTabs);
        setBlockedPopups((current) => {
          const next = forgetBlockedPopups(current, pageId);
          blockedPopupsRef.current = next;
          return next;
        });
        const nextTab = nextTabs.find((tab) => tab.id === nextId);
        if (nextTab) {
          setActivePageId(nextTab.id);
          if (typeof nextTab.url === "string") setUrlFromPage(nextTab.url);
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
    [adapter, compact, currentPageId, sessionId, setActivePageId, setUrlFromPage, syncBrowserViewportForPage, tabs, updateSnapshot]
  );

  const openFromOpener = useCallback(async (openerId, url, background) => {
    const activeSession = sessionIdRef.current;
    if (!activeSession) return;
    let pageId;
    try {
      pageId = await adapter.createPage(activeSession, background ? { activate: false } : undefined);
    } catch (error) {
      setStatus(`New tab failed: ${messageText(error)}`);
      return;
    }
    setTabs((items) => insertOpenedTab(items, openerId, pageId, url));
    if (!background) {
      setActivePageId(pageId);
      setUrlFromPage(url || "");
      await syncBrowserViewportForPage(pageId);
      if (url) {
        setStatus(`Navigating to ${url}`);
        setLoading(true);
      } else {
        setStatus("New tab ready");
        if (!compact) document.querySelector(".location .url-input")?.focus();
      }
    } else {
      await syncBrowserViewportForPage(pageId);
    }
    if (!url) return;
    try {
      await adapter.navigate(activeSession, pageId, url);
      if (!background && !compact) {
        try {
          const nextSnapshot = await adapter.getPageSnapshot(activeSession, pageId);
          updateSnapshot(pageId, nextSnapshot);
        } catch (error) {
          setStatus(`Page refresh failed: ${messageText(error)}`);
        }
      }
    } catch (error) {
      setStatus(`Navigation failed: ${messageText(error)}`);
    } finally {
      if (!background) {
        setLoading(null);
        setPageLoading(false);
      }
    }
  }, [adapter, compact, setActivePageId, setUrlFromPage, syncBrowserViewportForPage, updateSnapshot]);

  const enqueuePopup = useCallback((job) => {
    popupQueue.current = popupQueue.current.then(job, job);
  }, []);

  const openBlockedPopup = useCallback(() => {
    const pageId = currentPageIdRef.current;
    if (pageId == null) return;
    const taken = takeBlockedPopup(blockedPopupsRef.current, pageId);
    blockedPopupsRef.current = taken.popups;
    setBlockedPopups(taken.popups);
    if (taken.url == null) return;
    enqueuePopup(() => openFromOpener(pageId, taken.url, false));
  }, [enqueuePopup, openFromOpener]);

  const dismissBlockedPopup = useCallback(() => {
    const pageId = currentPageIdRef.current;
    if (pageId == null) return;
    setBlockedPopups((current) => {
      const next = forgetBlockedPopups(current, pageId);
      blockedPopupsRef.current = next;
      return next;
    });
  }, []);

  const reorderTabs = useCallback((fromIndex, toIndex) => {
    setTabs((items) => moveTab(items, fromIndex, toIndex));
  }, []);

  const navigateCurrentPage = useCallback(async (nextUrl) => {
    if (currentPageId == null || !sessionId) return;
    // The host navigate command still runs netguard on this URL.
    const destination = classifyOmniboxInput(typeof nextUrl === "string" ? nextUrl : url);
    if (!destination) return;

    const token = commandLoadToken.current + 1;
    commandLoadToken.current = token;
    setUrlFromPage(destination.url);
    setStatus(
      destination.kind === "search"
        ? `Searching DuckDuckGo for ${destination.query}`
        : `Navigating to ${destination.url}`
    );
    setLoading(true);
    expectNativeNavigation(currentPageId);

    try {
      await performPageAction(
        () => adapter.navigate(sessionId, currentPageId, destination.url),
        "Navigation failed"
      );
    } finally {
      if (commandLoadToken.current === token) {
        setLoading(null);
        setPageLoading(false);
      }
    }
  }, [adapter, currentPageId, expectNativeNavigation, performPageAction, sessionId, setUrlFromPage, url]);

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

  const stopPageLoad = useCallback(async () => {
    if (currentPageId == null || !sessionId) return;
    try {
      await adapter.browserAction("stop_page_load", sessionId, currentPageId);
      setLoading(null);
      setPageLoading(false);
    } catch (error) {
      setStatus(`Stop failed: ${messageText(error)}`);
    }
  }, [adapter, currentPageId, sessionId]);

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
        setTabs([{ id: pageId, title: "Tab 1", url: "" }]);
        setActivePageId(pageId);
        if (!compact) document.querySelector(".location .url-input")?.focus();
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
          if ("url" in updates) setUrlFromPage(updates.url);
        }
        // Status follows the active tab's newest WebKit navigation only.
        const next = nativeStatusUpdate(nativeNavigationRef.current, event, currentPageIdRef.current);
        if ("url" in next) setUrlFromPage(next.url);
        if (next.snapshot) updateSnapshot(event.pageId, next.snapshot);
        else if (next.status) setStatus(next.status);
      }
    });
    return unsubscribe;
  }, [adapter, compact, setActivePageId, setUrlFromPage, updateSnapshot]);

  useEffect(() => {
    const onFocusOut = (event) => {
      if (event.target?.classList?.contains("url-input")) addressDirtyRef.current = false;
    };
    document.addEventListener("focusout", onFocusOut);
    return () => document.removeEventListener("focusout", onFocusOut);
  }, []);

  useEffect(() => {
    if (typeof adapter.onPageLifecycle !== "function") return undefined;
    return adapter.onPageLifecycle((event) => {
      const updates = pageLifecycleUpdates(event, currentPageIdRef.current, {
        addressEditing: addressDirtyRef.current,
      });
      if (!updates.phase) return;
      if (updates.loading === true) setPageLoading(true);
      if (updates.loading === false) setPageLoading(false);
      if (typeof updates.url === "string") setUrlFromPage(updates.url);
      if (updates.phase === "finished" && updates.pageId === currentPageIdRef.current) {
        refreshZoomRef.current(updates.pageId);
      }
      const pageId = updates.pageId;
      const title = typeof updates.title === "string" ? updates.title : null;
      const nextPageUrl = typeof event.url === "string" && event.url ? event.url : null;
      if (title || nextPageUrl) {
        setTabs((items) =>
          items.map((tab) => {
            if (tab.id !== pageId) return tab;
            return {
              ...tab,
              ...(title ? { title } : {}),
              ...(nextPageUrl ? { url: nextPageUrl } : {}),
            };
          })
        );
      }
    });
  }, [adapter, setUrlFromPage]);

  useEffect(() => {
    if (typeof adapter.onPageNewWindow !== "function") return undefined;
    return adapter.onPageNewWindow((event) => {
      const request = pageNewWindowRequest(event);
      if (!request) return;
      if (request.disposition === "blocked") {
        setBlockedPopups((current) => {
          const next = rememberBlockedPopup(current, request.pageId, request.url);
          blockedPopupsRef.current = next;
          return next;
        });
        return;
      }
      enqueuePopup(() => openFromOpener(
        request.pageId,
        request.url,
        request.disposition === "background",
      ));
    });
  }, [adapter, enqueuePopup, openFromOpener]);

  useEffect(() => {
    if (typeof adapter.onDownload !== "function") return undefined;
    return adapter.onDownload((event) => {
      setDownloads((current) => applyDownloadUpdate(current, event));
    });
  }, [adapter]);

  const openDownload = useCallback(async (id) => {
    if (typeof adapter.openDownload !== "function") return;
    try {
      await adapter.openDownload(id);
    } catch (error) {
      setStatus(`Could not open download: ${messageText(error)}`);
    }
  }, [adapter]);

  const showDownload = useCallback(async (id) => {
    if (typeof adapter.showDownloadInFolder !== "function") return;
    try {
      await adapter.showDownloadInFolder(id);
    } catch (error) {
      setStatus(`Could not show download: ${messageText(error)}`);
    }
  }, [adapter]);

  const refreshZoom = useCallback(async (pageId = currentPageId) => {
    if (compact || typeof adapter.adjustPageZoom !== "function") return;
    if (pageId == null || sessionId == null) return;
    try {
      const result = await adapter.adjustPageZoom(sessionId, pageId, "get");
      if (pageId === currentPageIdRef.current && Number.isFinite(result?.percent)) {
        setZoomPercent(result.percent);
      }
    } catch (error) {
      console.warn("zoom read failed", messageText(error));
    }
  }, [adapter, compact, currentPageId, sessionId]);
  const refreshZoomRef = useRef(refreshZoom);
  refreshZoomRef.current = refreshZoom;

  const changeZoom = useCallback(async (action) => {
    if (compact || typeof adapter.adjustPageZoom !== "function") return;
    if (currentPageId == null || sessionId == null) return;
    try {
      const result = await adapter.adjustPageZoom(sessionId, currentPageId, action);
      if (Number.isFinite(result?.percent)) setZoomPercent(result.percent);
    } catch (error) {
      setStatus(`Zoom failed: ${messageText(error)}`);
    }
  }, [adapter, compact, currentPageId, sessionId]);

  const runFind = useCallback(async (query, direction, pageId = currentPageId) => {
    if (compact || typeof adapter.findInPage !== "function") return;
    if (pageId == null || sessionId == null) return;
    const token = ++findToken.current;
    try {
      const result = await adapter.findInPage(sessionId, pageId, query, direction);
      if (token !== findToken.current || pageId !== currentPageIdRef.current) return;
      setFindHit({
        total: Number(result?.total) || 0,
        current: Number(result?.current) || 0,
      });
    } catch (error) {
      if (token !== findToken.current || pageId !== currentPageIdRef.current) return;
      setStatus(`Find failed: ${messageText(error)}`);
    }
  }, [adapter, compact, currentPageId, sessionId]);

  const openFind = useCallback(() => {
    setFindOpen(true);
    const focusField = () => {
      findInputRef.current?.focus();
      findInputRef.current?.select();
    };
    if (typeof window.requestAnimationFrame === "function") {
      window.requestAnimationFrame(focusField);
    } else {
      queueMicrotask(focusField);
    }
  }, []);

  const closeFind = useCallback(() => {
    setFindOpen(false);
    setFindQuery("");
    setFindHit({ total: 0, current: 0 });
    runFind("", "start");
  }, [runFind]);

  const onFindQuery = useCallback((value) => {
    setFindQuery(value);
    runFind(value, "start");
  }, [runFind]);

  const closeFindRef = useRef(closeFind);
  closeFindRef.current = closeFind;
  const chromeActionRef = useRef(() => {});
  chromeActionRef.current = (shortcut) => {
    if (shortcut === "find") {
      openFind();
      return;
    }
    if (shortcut === "find-next" || shortcut === "find-previous") {
      setFindOpen(true);
      const query = findQueryRef.current;
      const direction = !query ? "start" : shortcut === "find-next" ? "next" : "previous";
      runFind(query, direction);
      return;
    }
    if (shortcut === "zoom-in") changeZoom("in");
    else if (shortcut === "zoom-out") changeZoom("out");
    else if (shortcut === "zoom-reset") changeZoom("reset");
  };

  const activeTab = tabs.find((tab) => tab.id === currentPageId);
  const showStartPage =
    !compact &&
    activeTab != null &&
    isBlankDocument(activeTab.url) &&
    loading == null &&
    !pageLoading;

  useEffect(() => {
    if (!adapter.rendersPageInHost) return undefined;
    let frame = null;
    let frameKind = null;
    const queueSync = () => {
      if (frame != null) return;
      const run = async () => {
        frame = null;
        frameKind = null;
        try {
          await syncBrowserViewport();
        } catch (error) {
          console.warn("viewport sync failed", messageText(error));
        }
      };
      if (typeof window.requestAnimationFrame === "function") {
        frameKind = "animation";
        frame = window.requestAnimationFrame(run);
      } else {
        frameKind = "timeout";
        frame = setTimeout(run, 16);
      }
    };
    window.addEventListener("resize", queueSync);
    const observer = typeof ResizeObserver === "function" ? new ResizeObserver(queueSync) : null;
    if (browserStageRef.current) observer?.observe(browserStageRef.current);
    queueSync();
    return () => {
      window.removeEventListener("resize", queueSync);
      observer?.disconnect();
      if (frame == null) return;
      if (frameKind === "animation") window.cancelAnimationFrame(frame);
      else clearTimeout(frame);
    };
  }, [adapter.rendersPageInHost, syncBrowserViewport]);

  useEffect(() => {
    // Keys from the page webview never arrive here. The native menu emits the
    // same commands when that webview is focused. Marking a chord here drops
    // the menu echo of this key so chrome focus does not run it twice.
    const onKeyDown = (event) => {
      if (event.key === "Escape") {
        if (findOpenRef.current && !compact && !event.repeat && !event.altKey && !event.metaKey && !event.ctrlKey) {
          event.preventDefault();
          closeFindRef.current();
          return;
        }
        if (compact || event.repeat || event.altKey || event.metaKey || event.ctrlKey) return;
        menuEcho.current.mark("stop");
        if (event.defaultPrevented) return;
        if (loading == null && !pageLoading) return;
        if (addressDirtyRef.current) return;
        if (currentPageId == null || sessionId == null) return;
        const addressFocused =
          event.target?.classList?.contains("url-input") ||
          document.activeElement?.classList?.contains("url-input");
        if (!addressFocused && typingInTextField(event)) return;
        event.preventDefault();
        stopPageLoad();
        return;
      }

      const altHistory =
        event.altKey &&
        !event.metaKey &&
        !event.ctrlKey &&
        !event.shiftKey &&
        (event.key === "ArrowLeft" ||
          event.key === "ArrowRight" ||
          event.code === "ArrowLeft" ||
          event.code === "ArrowRight");
      if (altHistory) {
        if (compact) return;
        const forward = event.key === "ArrowRight" || event.code === "ArrowRight";
        menuEcho.current.mark(forward ? "forward" : "back");
        if (event.repeat) return;
        event.preventDefault();
        runBrowserAction(forward ? "browser_forward" : "browser_back");
        return;
      }

      const shortcut = chromeShortcut(event);
      if (shortcut && !compact) {
        menuEcho.current.mark(shortcut);
        event.preventDefault();
        chromeActionRef.current(shortcut);
        return;
      }

      const modifier = event.metaKey || event.ctrlKey;
      if (event.key === "Tab" && event.ctrlKey && !event.metaKey && !event.altKey) {
        if (compact) return;
        menuEcho.current.mark(event.shiftKey ? "previous-tab" : "next-tab");
        event.preventDefault();
        activateRelativeTab(tabs, currentPageId, event.shiftKey ? -1 : 1, activatePage);
        return;
      }
      if (!modifier) return;
      const key = event.key.toLowerCase();
      if (key === "t") {
        if (compact) return;
        menuEcho.current.mark("new-tab");
        event.preventDefault();
        createNewTab();
        return;
      }
      if (key === "l") {
        menuEcho.current.mark("focus-address");
        event.preventDefault();
        document.querySelector(".url-input")?.focus();
        return;
      }
      if (key === "w" && currentPageId != null) {
        if (compact) return;
        menuEcho.current.mark("close-tab");
        event.preventDefault();
        closeTab(currentPageId);
        return;
      }

      if (compact || event.altKey) return;

      if (key === "r" && !event.shiftKey) {
        menuEcho.current.mark("reload");
        if (event.repeat) return;
        event.preventDefault();
        runBrowserAction("browser_reload");
        return;
      }
      if (typingInTextField(event)) {
        const side = bracketSide(event);
        if (side) {
          const shifted = event.shiftKey || event.key === "{" || event.key === "}";
          menuEcho.current.mark(
            shifted
              ? side === "left"
                ? "previous-tab"
                : "next-tab"
              : side === "left"
                ? "back"
                : "forward"
          );
        } else if (!event.shiftKey && !event.repeat && /^[1-9]$/.test(event.key)) {
          menuEcho.current.mark(`select-tab-${event.key}`);
        }
        return;
      }

      const side = bracketSide(event);
      if (side) {
        const shifted = event.shiftKey || event.key === "{" || event.key === "}";
        menuEcho.current.mark(
          shifted ? (side === "left" ? "previous-tab" : "next-tab") : side === "left" ? "back" : "forward"
        );
        event.preventDefault();
        if (shifted) {
          activateRelativeTab(tabs, currentPageId, side === "left" ? -1 : 1, activatePage);
        } else if (!event.repeat) {
          runBrowserAction(side === "left" ? "browser_back" : "browser_forward");
        }
        return;
      }

      if (event.shiftKey || !/^[1-9]$/.test(event.key)) return;
      menuEcho.current.mark(`select-tab-${event.key}`);
      if (event.repeat) return;
      event.preventDefault();
      activateNumberedTab(tabs, currentPageId, Number(event.key), activatePage);
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [activatePage, closeTab, compact, createNewTab, currentPageId, loading, pageLoading, runBrowserAction, sessionId, stopPageLoad, tabs]);

  useEffect(() => {
    if (typeof adapter.onMenuCommand !== "function") return undefined;
    return adapter.onMenuCommand((command) => {
      if (menuEcho.current.isEcho(command)) return;
      if (compact && command !== "focus-address") return;
      if (command === "new-tab") {
        createNewTab();
        return;
      }
      if (command === "close-tab") {
        if (currentPageId != null) closeTab(currentPageId);
        return;
      }
      if (command === "focus-address") {
        document.querySelector(".url-input")?.focus();
        return;
      }
      if (command === "find" || command === "find-next" || command === "find-previous" || command === "zoom-in" || command === "zoom-out" || command === "zoom-reset") {
        chromeActionRef.current(command);
        return;
      }
      if (command === "reload") {
        runBrowserAction("browser_reload");
        return;
      }
      if (command === "stop") {
        if (loading == null && !pageLoading) return;
        if (addressDirtyRef.current) return;
        if (currentPageId == null || sessionId == null) return;
        stopPageLoad();
        return;
      }
      if (command === "back" || command === "forward") {
        runBrowserAction(command === "back" ? "browser_back" : "browser_forward");
        return;
      }
      if (command === "next-tab" || command === "previous-tab") {
        activateRelativeTab(tabs, currentPageId, command === "previous-tab" ? -1 : 1, activatePage);
        return;
      }
      const number = numberedTabCommand(command);
      if (number == null) return;
      activateNumberedTab(tabs, currentPageId, number, activatePage);
    });
  }, [activatePage, adapter, closeTab, compact, createNewTab, currentPageId, loading, pageLoading, runBrowserAction, sessionId, stopPageLoad, tabs]);

  const blockedCount = Array.isArray(blockedPopups[currentPageId]) ? blockedPopups[currentPageId].length : 0;

  const stageText =
    "Live page webview attaches here. This React surface controls layout, browser commands, and AI actions while the actual page renders in a real child webview.";

  if (compact) {
    return (
      <div className="app-shell appkit-shell">
        <LoadProgress active={loading != null || pageLoading} />
        <Header
          adapter={adapter}
          blockedCount={blockedCount}
          compact
          onDismissBlocked={dismissBlockedPopup}
          onNavigate={navigateCurrentPage}
          onNewTab={createNewTab}
          onOpenBlocked={openBlockedPopup}
          onStatus={setStatus}
          setUrl={setUrlFromUser}
          url={url}
        />
        <TabStrip currentPageId={currentPageId} onActivate={activatePage} onClose={closeTab} onReorder={reorderTabs} tabs={tabs} />
        <BrowserToolbar loading={false} onAction={runBrowserAction} onStop={stopPageLoad} />
        <StatsRow snapshot={snapshot} />
        <div className="status-bar">
          <span className="status-line">{status}</span>
          <span>Tabs: {tabs.length}</span>
        </div>
      </div>
    );
  }

  return (
    <div className="app-shell">
      <LoadProgress active={loading != null || pageLoading} />
      <Header
        adapter={adapter}
        blockedCount={blockedCount}
        onDismissBlocked={dismissBlockedPopup}
        onNavigate={navigateCurrentPage}
        onNewTab={createNewTab}
        onOpenBlocked={openBlockedPopup}
        onStatus={setStatus}
        policyMode={policyMode}
        provider={provider}
        setPolicyMode={selectPolicyMode}
        setProvider={selectProvider}
        setUrl={setUrlFromUser}
        url={url}
      />
      <TabStrip currentPageId={currentPageId} onActivate={activatePage} onClose={closeTab} onReorder={reorderTabs} tabs={tabs} />
      <main>
        <section className="browser-panel">
          <BrowserToolbar loading={loading != null || pageLoading} onAction={runBrowserAction} onStop={stopPageLoad} zoomPercent={zoomPercent} />
          {findOpen ? (
            <FindBar
              current={findHit.current}
              inputRef={findInputRef}
              onClose={closeFind}
              onNext={() => runFind(findQuery, findQuery ? "next" : "start")}
              onPrevious={() => runFind(findQuery, findQuery ? "previous" : "start")}
              onQuery={onFindQuery}
              query={findQuery}
              total={findHit.total}
            />
          ) : null}
          <StatsRow snapshot={snapshot} />
          <div className="downloads-slot">
            <Downloads items={downloads} onOpen={openDownload} onShow={showDownload} />
          </div>
          <div className="browser-stage" ref={browserStageRef}>
            {showStartPage ? (
              <NewTabPage adapter={adapter} onNavigate={navigateCurrentPage} />
            ) : (
              <div className="stage-placeholder">
                <h2>{snapshot?.title || "Native page rendering"}</h2>
                <p>{stageText}</p>
              </div>
            )}
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
        <span className="status-line">{status}</span>
        <span>Tabs: {tabs.length}</span>
      </div>
    </div>
  );
}
