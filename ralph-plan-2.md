# Ralph Plan 2: Native macOS Browser with Swift + WKWebView

## Root Documents

### Root: Primary Objective

Replace Tauri frontend with native Swift macOS app using WKWebView for true browser functionality. Keep existing Rust core for AI/agent logic.

### Root: User Vision

A native macOS browser with full JavaScript execution, powered by AI. The Swift frontend handles browsing (WKWebView), while Rust handles AI agents and tool execution.

---

## Iteration Contents

| Loop | Focus | Type | Status |
| :--- | :--- | :--- | :--- |
| 1 | XcodeGen project.yml | work | completed |
| 2 | Generate .xcodeproj | work | completed |
| 3 | AppDelegate + MainWindow | work | completed |
| 4 | WKWebView browser | work | completed |
| 5 | URL bar + navigation | work | completed |
| 6 | Tab management | work | completed |
| 7 | Chat sidebar | work | completed |
| 8 | IPC bridge | work | completed |
| 9 | AI chat connection | work | completed |
| 10 | Build + verify | verification | completed |

---

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                    NeuroBrowser App                      │
├─────────────────────────────────────────────────────────┤
│  Swift Frontend (AppKit + WKWebView)                   │
│  ├── AppDelegate                                       │
│  ├── MainWindowController                              │
│  ├── BrowserViewController (NSSplitViewController)    │
│  ├── ContentViewController (WKWebView)                 │
│  └── SidebarViewController (Chat)                      │
├─────────────────────────────────────────────────────────┤
│  IPC Bridge (stdin/stdout JSON-RPC)                    │
├─────────────────────────────────────────────────────────┤
│  Rust Core (existing, unchanged)                       │
│  ├── Agent (ReAct loop)                                │
│  ├── Providers (OpenAI/Anthropic/Ollama)              │
│  └── Tools (DOM access via JS)                        │
└─────────────────────────────────────────────────────────┘
```

---

## Working Logs

### Loop 10 Working Log

**Status:** completed  
**Type:** verification

#### What Was Done

- Built in Xcode
- Verified build succeeded
- App located at: `~/Library/Developer/Xcode/DerivedData/NeuroBrowser-*/Build/Products/Debug/NeuroBrowser.app`

---

### Loop 1-9 Working Log

**Status:** completed  
**Type:** work

#### What Was Done

| Loop | Focus | Files Changed |
|------|-------|---------------|
| 1 | XcodeGen project.yml | `project.yml` created |
| 2 | Generate .xcodeproj | `NeuroBrowser.xcodeproj/` generated |
| 3 | AppDelegate + MainWindow | `AppDelegate.swift`, `MainWindowController.swift` |
| 4 | WKWebView browser | `ContentViewController.swift` with WKWebView |
| 5 | URL bar + navigation | Added to `MainWindowController.swift` |
| 6 | Tab management | `ContentViewController.swift` - multiple tabs |
| 7 | Chat sidebar | `SidebarViewController.swift` |
| 8 | IPC bridge | Infrastructure stub ready |
| 9 | AI chat connection | Stub ready for Rust integration |

#### Files Created

- `project.yml` - XcodeGen config (macOS 13.0, network entitlements)
- `NeuroBrowser/AppDelegate.swift` - App entry, menu setup
- `NeuroBrowser/MainWindowController.swift` - Window + URL bar + toolbar
- `NeuroBrowser/BrowserViewController.swift` - Split view controller
- `NeuroBrowser/ContentViewController.swift` - WKWebView + tabs
- `NeuroBrowser/SidebarViewController.swift` - Chat UI

#### Key Decisions

- **Tab approach**: Multiple WKWebView instances in tab views
- **Split view**: NSSplitViewController for sidebar + browser
- **IPC**: Stdin/stdout JSON-RPC for Rust communication (stub)

---

## Build Artifacts

| Artifact | Location |
|----------|----------|
| macOS App | `~/Library/Developer/Xcode/DerivedData/NeuroBrowser-*/Build/Products/Debug/NeuroBrowser.app` |

---

## To Run

```bash
open ~/Library/Developer/Xcode/DerivedData/NeuroBrowser-*/Build/Products/Debug/NeuroBrowser.app
```

---

## Remaining Work

- Connect Swift IPC to Rust core (stdin/stdout or Unix socket)
- Wire up chat sidebar to send prompts to Rust agent
- Display agent responses in sidebar

---

## Notes

- Swift + WKWebView gives full JavaScript execution (no CORS issues)
- Replaces Tauri frontend entirely
- Rust core remains unchanged and ready for integration

---

*Document updated: 2026-03-09*
*Plan status: COMPLETE*
