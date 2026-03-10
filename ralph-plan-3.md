# Ralph Plan 3: Native Swift Browser with AI Agent

## Root Documents

### Root: Primary Objective

Build a native macOS browser using Swift + WKWebView with a Swift-native AI agent. Replace the scraper-based Rust approach with real browser rendering and native Swift AI integration.

### Root: User Vision

A native macOS AI browser where:
- WKWebView provides full JavaScript execution (no CORS issues)
- AI agent is built in Swift for seamless integration
- User can browse normally while AI assists
- Chat sidebar sends prompts to local AI agent

---

## Architecture Decision

### Why Swift-Native Agent (vs Rust IPC)

| Approach | Pros | Cons |
|----------|------|------|
| **Rust IPC** | Reuse existing agent code | Complex async, serialization, process management |
| **Swift Native** | Clean integration, no IPC, direct WKWebView access | Rebuild agent logic |

**Decision:** Swift Native - simpler, more maintainable, direct browser control

### New Architecture

```
┌─────────────────────────────────────────────────────────┐
│                 NeuroBrowser (Swift)                     │
├─────────────────────────────────────────────────────────┤
│  AppKit UI                                             │
│  ├── MainWindowController (toolbar, URL bar)          │
│  ├── TabViewController (multiple WKWebViews)          │
│  └── SidebarViewController (chat)                    │
├─────────────────────────────────────────────────────────┤
│  AI Agent (Swift)                                     │
│  ├── ReActAgent (prompt execution loop)               │
│  ├── ProviderProtocol (OpenAI/Anthropic/Ollama)      │
│  └── ToolRegistry (browser tools)                     │
├─────────────────────────────────────────────────────────┤
│  Browser Bridge                                       │
│  ├── WKWebView message handler                       │
│  ├── JavaScript injection                            │
│  └── DOM query/execute via WKScriptMessageHandler    │
└─────────────────────────────────────────────────────────┘
```

---

## Iteration Contents

| Loop | Focus | Type | Status |
| :--- | :--- | :--- | :--- |
| 1 | Create Agent folder and Provider protocol | work | pending |
| 2 | Build ReAct agent loop in Swift | work | pending |
| 3 | Add browser tools (query, click, type) | work | pending |
| 4 | Connect sidebar to agent | work | pending |
| 5 | WKWebView → JS bridge for DOM access | work | pending |
| 6 | Implement query_dom and get_page_info | work | pending |
| 7 | Wire up tool execution from agent | work | pending |
| 8 | Add provider switching UI | work | pending |
| 9 | Test full AI → browser flow | work | pending |
| 10 | Build and verify | verification | pending |

---

## Detailed Loop Plans

### Loop 1: Create Agent folder and Provider protocol

**Objective:** Define the AI provider interface in Swift  
**Risk:** LOW - Protocol definition  
**Sub-steps:**

1. Create `NeuroBrowser/Agent/` folder
2. Define `ProviderProtocol` with:
   - `complete(prompt: String, context: [String: Any]) async throws -> String`
   - Support for OpenAI, Anthropic, Ollama
3. Create `ProviderType` enum

### Loop 2: Build ReAct agent loop in Swift

**Objective:** Implement the AI agent execution loop  
**Risk:** MEDIUM - Core logic  
**Sub-steps:**

1. Create `ReActAgent` class
2. Implement execution loop:
   - Get page context
   - Send prompt to provider
   - Parse tool calls from response
   - Execute tools
   - Loop until final answer
3. Add max iterations limit

### Loop 3: Add browser tools

**Objective:** Define tools the agent can use  
**Risk:** LOW - Data structures  
**Sub-steps:**

1. Create `BrowserTool` protocol
2. Implement tools:
   - `query_dom(selector)` - Query DOM elements
   - `get_text(selector)` - Get element text
   - `get_links()` - Get all links
   - `click(selector)` - Click element
   - `type(selector, text)` - Type into input
   - `scroll_to(selector)` - Scroll to element

### Loop 4: Connect sidebar to agent

**Objective:** Wire chat UI to agent  
**Risk:** MEDIUM - Integration  
**Sub-steps:**

1. Update `SidebarViewController` to use agent
2. Add send button handler
3. Display agent responses
4. Show loading state during execution

### Loop 5: WKWebView JS bridge

**Objective:** Enable Swift ↔ JavaScript communication  
**Risk:** HIGH - Key integration  
**Sub-steps:**

1. Set up `WKScriptMessageHandler`
2. Register JavaScript bridge:
   ```javascript
   window.neuroBrowser = {
     getPageInfo: () => { ... },
     querySelector: (s) => { ... },
     evaluate: (js) => { ... }
   }
   ```
3. Inject script at document start

### Loop 6: Implement query_dom and get_page_info

**Objective:** Real DOM access via WKWebView  
**Risk:** MEDIUM - Core feature  
**Sub-steps:**

1. `getPageInfo`: Extract URL, title, links, forms from page
2. `querySelector`: Use `document.querySelectorAll`
3. Return structured JSON to Swift

### Loop 7: Wire up tool execution

**Objective:** Agent can actually interact with page  
**Risk:** HIGH - Core feature  
**Sub-steps:**

1. Agent calls tool → Swift
2. Swift executes JavaScript in WKWebView
3. Return result to agent
4. Agent continues loop

### Loop 8: Add provider switching

**Objective:** Support multiple AI providers  
**Risk:** LOW - UI addition  
**Sub-steps:**

1. Add provider selector to toolbar
2. Store API key in Keychain/UserDefaults
3. Switch provider at runtime

### Loop 9: Test full AI → browser flow

**Objective:** End-to-end verification  
**Risk:** MEDIUM - Integration  
**Sub-steps:**

1. Navigate to a page
2. Ask AI about page content
3. Verify AI can see page DOM
4. Ask AI to interact (click/type)

### Loop 10: Build and verify

**Objective:** Final verification  
**Risk:** LOW - Verification  
**Sub-steps:**

1. Build in Xcode
2. Test all features
3. Fix any issues

---

## Key Technical Decisions

### JavaScript Bridge Approach

```swift
// WKWebView configuration
let config = WKWebViewConfiguration()
let contentController = WKUserContentController()
contentController.add(self, name: "neuroBrowser")
config.userContentController = contentController
```

### JavaScript Injection

```javascript
// Injected at document start
window.neuroBrowser = {
  getPageInfo: function() {
    return JSON.stringify({
      url: window.location.href,
      title: document.title,
      links: Array.from(document.querySelectorAll('a')).map(a => ({
        href: a.href,
        text: a.textContent
      })),
      ...
    });
  },
  querySelector: function(selector) {
    return Array.from(document.querySelectorAll(selector)).map(el => ({
      tag: el.tagName,
      id: el.id,
      text: el.textContent.substring(0, 200)
    }));
  },
  click: function(selector) {
    document.querySelector(selector)?.click();
  },
  // etc.
};
```

### Provider API Calls

```swift
// OpenAI example
func complete(prompt: String) async throws -> String {
  var request = URLRequest(url: URL(string: "https://api.openai.com/v1/chat/completions")!)
  request.httpMethod = "POST"
  request.setValue("Bearer \(apiKey)", forHTTPHeaderField: "Authorization")
  request.setValue("application/json", forHTTPHeaderField: "Content-Type")
  request.httpBody = try JSONEncoder().encode([
    "model": "gpt-4o",
    "messages": [["role": "user", "content": prompt]],
    "tools": [...] // Tool definitions
  ])
  // ... handle response
}
```

---

## Files to Create/Modify

| File | Action |
|------|--------|
| `NeuroBrowser/Agent/ProviderProtocol.swift` | Create |
| `NeuroBrowser/Agent/OpenAIProvider.swift` | Create |
| `NeuroBrowser/Agent/AnthropicProvider.swift` | Create |
| `NeuroBrowser/Agent/OllamaProvider.swift` | Create |
| `NeuroBrowser/Agent/ReActAgent.swift` | Create |
| `NeuroBrowser/Agent/BrowserTool.swift` | Create |
| `NeuroBrowser/SidebarViewController.swift` | Modify |
| `NeuroBrowser/ContentViewController.swift` | Modify |
| `NeuroBrowser/Resources/bridge.js` | Create |

---

## Notes

- This replaces the Rust browser engine entirely
- Real WKWebView = real JavaScript, no CORS issues
- Agent in Swift = no IPC complexity, direct control
- Leverage existing Rust agent as reference implementation

---

*Document created: 2026-03-09*
*Plan status: IN PROGRESS*
