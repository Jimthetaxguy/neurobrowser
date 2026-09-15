import AppKit
import WebKit

class ContentViewController: NSViewController {
    
    // MARK: - UI Components
    
    var urlBar: NSTextField!
    var backButton: NSButton!
    var forwardButton: NSButton!
    var reloadButton: NSButton!
    var webViews: [WKWebView] = []
    var tabBar: NSSegmentedControl!
    var pageUpdateHandler: (([String: Any]) -> Void)?
    private var webViewContainer: NSView!
    private var pageIds: [Int] = []
    
    // MARK: - State
    
    var currentTabIndex: Int = 0
    var currentPageId: Int? {
        pageIds.indices.contains(currentTabIndex) ? pageIds[currentTabIndex] : nil
    }
    
    // MARK: - Lifecycle
    
    override func loadView() {
        self.view = NSView(frame: NSRect(x: 0, y: 0, width: 900, height: 800))
    }
    
    override func viewDidLoad() {
        super.viewDidLoad()
        setupUI()
    }
    
    // MARK: - Setup
    
    private func setupUI() {
        view.wantsLayer = true
        view.layer?.backgroundColor = NSColor.windowBackgroundColor.cgColor
        
        let toolbarContainer = NSView()
        toolbarContainer.translatesAutoresizingMaskIntoConstraints = false
        toolbarContainer.wantsLayer = true
        toolbarContainer.layer?.backgroundColor = NSColor.controlBackgroundColor.cgColor
        view.addSubview(toolbarContainer)
        
        backButton = createNavButton(title: "◀", action: #selector(goBack))
        forwardButton = createNavButton(title: "▶", action: #selector(goForward))
        reloadButton = createNavButton(title: "↻", action: #selector(reloadCurrentPage))
        
        urlBar = NSTextField()
        urlBar.translatesAutoresizingMaskIntoConstraints = false
        urlBar.placeholderString = "Enter a URL or domain..."
        urlBar.font = NSFont.systemFont(ofSize: 13)
        urlBar.bezelStyle = .roundedBezel
        urlBar.target = self
        urlBar.action = #selector(urlBarAction)
        toolbarContainer.addSubview(backButton)
        toolbarContainer.addSubview(forwardButton)
        toolbarContainer.addSubview(reloadButton)
        toolbarContainer.addSubview(urlBar)
        
        tabBar = NSSegmentedControl()
        tabBar.translatesAutoresizingMaskIntoConstraints = false
        tabBar.segmentCount = 1
        tabBar.setLabel("+", forSegment: 0)
        tabBar.setWidth(100, forSegment: 0)
        tabBar.selectedSegment = -1
        tabBar.target = self
        tabBar.action = #selector(tabBarChanged)
        tabBar.segmentStyle = .rounded
        view.addSubview(tabBar)
        
        webViewContainer = NSView()
        webViewContainer.translatesAutoresizingMaskIntoConstraints = false
        view.addSubview(webViewContainer)
        
        NSLayoutConstraint.activate([
            toolbarContainer.topAnchor.constraint(equalTo: view.topAnchor),
            toolbarContainer.leadingAnchor.constraint(equalTo: view.leadingAnchor),
            toolbarContainer.trailingAnchor.constraint(equalTo: view.trailingAnchor),
            toolbarContainer.heightAnchor.constraint(equalToConstant: 40),
            
            backButton.leadingAnchor.constraint(equalTo: toolbarContainer.leadingAnchor, constant: 8),
            backButton.centerYAnchor.constraint(equalTo: toolbarContainer.centerYAnchor),
            backButton.widthAnchor.constraint(equalToConstant: 30),
            
            forwardButton.leadingAnchor.constraint(equalTo: backButton.trailingAnchor, constant: 4),
            forwardButton.centerYAnchor.constraint(equalTo: toolbarContainer.centerYAnchor),
            forwardButton.widthAnchor.constraint(equalToConstant: 30),
            
            reloadButton.leadingAnchor.constraint(equalTo: forwardButton.trailingAnchor, constant: 4),
            reloadButton.centerYAnchor.constraint(equalTo: toolbarContainer.centerYAnchor),
            reloadButton.widthAnchor.constraint(equalToConstant: 30),
            
            urlBar.leadingAnchor.constraint(equalTo: reloadButton.trailingAnchor, constant: 8),
            urlBar.trailingAnchor.constraint(equalTo: toolbarContainer.trailingAnchor, constant: -8),
            urlBar.centerYAnchor.constraint(equalTo: toolbarContainer.centerYAnchor),
            
            tabBar.topAnchor.constraint(equalTo: toolbarContainer.bottomAnchor, constant: 4),
            tabBar.leadingAnchor.constraint(equalTo: view.leadingAnchor, constant: 8),
            tabBar.trailingAnchor.constraint(equalTo: view.trailingAnchor, constant: -8),
            tabBar.heightAnchor.constraint(equalToConstant: 24),
            
            webViewContainer.topAnchor.constraint(equalTo: tabBar.bottomAnchor, constant: 4),
            webViewContainer.leadingAnchor.constraint(equalTo: view.leadingAnchor),
            webViewContainer.trailingAnchor.constraint(equalTo: view.trailingAnchor),
            webViewContainer.bottomAnchor.constraint(equalTo: view.bottomAnchor)
        ])
    }
    
    private func createNavButton(title: String, action: Selector) -> NSButton {
        let button = NSButton(title: title, target: self, action: action)
        button.translatesAutoresizingMaskIntoConstraints = false
        button.bezelStyle = .rounded
        button.isBordered = true
        return button
    }
    
    // MARK: - Tab Management
    
    func addNewTab(pageId: Int? = nil) {
        if let pageId, let existing = pageIds.firstIndex(of: pageId) {
            currentTabIndex = existing
            tabBar.selectedSegment = existing
            showCurrentTab()
            return
        }

        let config = WKWebViewConfiguration()
        let webView = WKWebView(frame: .zero, configuration: config)
        webView.navigationDelegate = self
        
        webViews.append(webView)
        pageIds.append(pageId ?? (pageIds.max() ?? -1) + 1)
        
        let newIndex = webViews.count - 1
        tabBar.segmentCount = webViews.count + 1
        tabBar.setLabel("Tab \(newIndex + 1)", forSegment: newIndex)
        tabBar.setWidth(80, forSegment: newIndex)
        
        tabBar.setLabel("+", forSegment: webViews.count)
        
        tabBar.selectedSegment = newIndex
        currentTabIndex = newIndex
        
        webView.frame = webViewContainer.bounds
        webView.autoresizingMask = [.width, .height]
        webViewContainer.addSubview(webView)
        showCurrentTab()
        
        if let url = URL(string: "https://www.example.com") {
            webView.load(URLRequest(url: url))
        }
        
        updateNavigationButtons()
    }
    
    func closeCurrentTab() {
        closePage(pageId: currentPageId)
    }

    func closePage(pageId: Int? = nil) {
        guard webViews.count > 1 else { return }
        let id = pageId ?? currentPageId
        guard let id, let index = pageIds.firstIndex(of: id) else { return }

        let webView = webViews[index]
        webView.removeFromSuperview()
        webViews.remove(at: index)
        pageIds.remove(at: index)

        if currentTabIndex > index {
            currentTabIndex -= 1
        } else if currentTabIndex == index {
            currentTabIndex = min(index, webViews.count - 1)
        }

        tabBar.segmentCount = webViews.count + 1
        tabBar.setLabel("+", forSegment: webViews.count)
        tabBar.selectedSegment = currentTabIndex

        showCurrentTab()
        updateNavigationButtons()
    }

    func selectTab(pageId: Int) {
        guard let index = pageIds.firstIndex(of: pageId) else { return }
        currentTabIndex = index
        tabBar.selectedSegment = index
        showCurrentTab()
    }

    func navigate(pageId: Int?, to input: String) {
        if let pageId {
            selectTab(pageId: pageId)
        }
        navigateCurrentTab(to: input)
    }
    
    @objc private func tabBarChanged() {
        let selected = tabBar.selectedSegment
        
        if selected == webViews.count {
            addNewTab()
            return
        }
        
        currentTabIndex = selected
        showCurrentTab()
    }
    
    private func showCurrentTab() {
        for webView in webViews {
            webView.isHidden = true
        }
        
        if currentTabIndex < webViews.count {
            let webView = webViews[currentTabIndex]
            webView.isHidden = false
            
            if let url = webView.url {
                urlBar.stringValue = url.absoluteString
            }
            
            updateNavigationButtons()
        }
    }
    
    // MARK: - Navigation
    
    @objc func goBack() {
        guard currentTabIndex < webViews.count else { return }
        webViews[currentTabIndex].goBack()
    }
    
    @objc func goForward() {
        guard currentTabIndex < webViews.count else { return }
        webViews[currentTabIndex].goForward()
    }
    
    @objc func reloadCurrentPage() {
        guard currentTabIndex < webViews.count else { return }
        webViews[currentTabIndex].reload()
    }
    
    @objc private func urlBarAction() {
        let input = urlBar.stringValue.trimmingCharacters(in: .whitespacesAndNewlines)
        navigateCurrentTab(to: input)
    }

    func navigateCurrentTab(to input: String) {
        let input = input.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !input.isEmpty, currentTabIndex < webViews.count else { return }
        guard let url = Self.validatedNavigationURL(from: input) else { return }
        urlBar.stringValue = url.absoluteString
        webViews[currentTabIndex].load(URLRequest(url: url))
    }

    private static func validatedNavigationURL(from input: String) -> URL? {
        let normalized: String
        if input.hasPrefix("http://") || input.hasPrefix("https://") {
            normalized = input
        } else if input.contains(".") && !input.contains(" ") {
            normalized = "https://" + input
        } else {
            return nil
        }
        guard let url = URL(string: normalized),
              let scheme = url.scheme?.lowercased(),
              scheme == "http" || scheme == "https" else {
            return nil
        }
        return url
    }

    func snapshotCurrentPage(completion: @escaping ([String: Any]) -> Void) {
        guard currentTabIndex < webViews.count else {
            completion(emptySnapshot())
            return
        }

        let webView = webViews[currentTabIndex]
        let script = """
        (() => {
          const text = document.body ? document.body.innerText : "";
          return {
            title: document.title || "",
            text: text.slice(0, 20000),
            link_count: document.links ? document.links.length : 0,
            image_count: document.images ? document.images.length : 0,
            form_count: document.forms ? document.forms.length : 0,
            price_count: (text.match(/\\$/g) || []).length,
            table_count: document.querySelectorAll ? document.querySelectorAll("table").length : 0
          };
        })()
        """

        webView.evaluateJavaScript(script) { result, _ in
            var snapshot = self.emptySnapshot()
            snapshot["url"] = webView.url?.absoluteString ?? ""

            if let pageData = result as? [String: Any] {
                snapshot.merge(pageData) { _, new in new }
            }

            completion(snapshot)
        }
    }

    private func emptySnapshot() -> [String: Any] {
        return [
            "url": "",
            "title": "",
            "html": "",
            "text": "",
            "link_count": 0,
            "image_count": 0,
            "form_count": 0,
            "price_count": 0,
            "table_count": 0,
            "viewport_width": Int(view.bounds.width),
            "viewport_height": Int(view.bounds.height),
            "interactive_ready": true
        ]
    }

    private func emitSnapshot() {
        snapshotCurrentPage { [weak self] snapshot in
            guard let self else { return }
            self.pageUpdateHandler?([
                "type": "snapshot",
                "pageId": self.currentPageId ?? 0,
                "snapshot": snapshot
            ])
        }
    }
    
    private func updateNavigationButtons() {
        guard currentTabIndex < webViews.count else { return }
        let webView = webViews[currentTabIndex]
        backButton.isEnabled = webView.canGoBack
        forwardButton.isEnabled = webView.canGoForward
    }
}

// MARK: - WKNavigationDelegate

extension ContentViewController: WKNavigationDelegate {
    
    func webView(_ webView: WKWebView, didStartProvisionalNavigation navigation: WKNavigation!) {
        reloadButton.title = "◌"
    }
    
    func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
        if let url = webView.url {
            urlBar.stringValue = url.absoluteString
        }
        reloadButton.title = "↻"
        updateNavigationButtons()
        emitSnapshot()
    }
    
    func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: Error) {
        reloadButton.title = "↻"
        updateNavigationButtons()
    }
    
    func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: Error) {
        reloadButton.title = "↻"
        updateNavigationButtons()
    }
}
