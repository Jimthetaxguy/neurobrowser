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
    // React allocates nonnegative IDs; native menu tabs use a disjoint sequence.
    private var nextNativePageId = -1
    
    // MARK: - State
    
    var currentTabIndex: Int = 0
    /// Every main-frame navigation WebKit starts gets the next id. Events carry
    /// the page and that id, so the control surface can drop results from an
    /// older load or another tab.
    private var nextNavigationId = 0
    private var latestNavigationIds: [ObjectIdentifier: Int] = [:]
    // Weak object keys cannot mistake a recycled WKNavigation address for an
    // earlier request that WebKit discarded without a terminal callback.
    private let navigationIds = NSMapTable<WKNavigation, NSNumber>(keyOptions: .weakMemory, valueOptions: .strongMemory)
    private var navigationStatuses: [ObjectIdentifier: String] = [:]
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
        tabBar.target = self
        tabBar.action = #selector(tabBarChanged)
        tabBar.segmentStyle = .rounded
        rebuildTabSegments()
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
        if let pageId {
            pageIds.append(pageId)
        } else {
            pageIds.append(nextNativePageId)
            nextNativePageId -= 1
        }
        
        currentTabIndex = webViews.count - 1
        rebuildTabSegments()
        
        webView.frame = webViewContainer.bounds
        webView.autoresizingMask = [.width, .height]
        webViewContainer.addSubview(webView)
        showCurrentTab()
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
        latestNavigationIds.removeValue(forKey: ObjectIdentifier(webView))
        navigationStatuses.removeValue(forKey: ObjectIdentifier(webView))
        webView.navigationDelegate = nil
        webView.stopLoading()
        webView.removeFromSuperview()
        webViews.remove(at: index)
        pageIds.remove(at: index)

        if currentTabIndex > index {
            currentTabIndex -= 1
        } else if currentTabIndex == index {
            currentTabIndex = min(index, webViews.count - 1)
        }

        rebuildTabSegments()

        showCurrentTab()
        updateNavigationButtons()
    }

    @discardableResult
    func selectTab(pageId: Int) -> Bool {
        guard let index = pageIds.firstIndex(of: pageId) else { return false }
        tabBar.selectedSegment = index
        // Commands for the tab that is already shown are not a tab switch. A
        // selection snapshot here would describe the page before the command ran.
        guard index != currentTabIndex else { return true }
        currentTabIndex = index
        showCurrentTab()
        return true
    }

    func navigate(pageId: Int?, to input: String) {
        if let pageId, !selectTab(pageId: pageId) { return }
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
            reloadButton.title = webView.isLoading ? "◌" : "↻"
            
            if let url = webView.url {
                urlBar.stringValue = url.absoluteString
            }
            
            updateNavigationButtons()
            emitTabs()
            emitSnapshot(reason: "select")
        }
    }

    /// `NSSegmentedControl` removes segments from the end when `segmentCount`
    /// shrinks, so incremental label edits leave the wrong title on a page.
    /// Paint every segment from `pageIds` / `webViews`, with `+` last.
    private func rebuildTabSegments() {
        let pages = Array(zip(pageIds, webViews))
        tabBar.segmentCount = pages.count + 1
        for (index, (_, webView)) in pages.enumerated() {
            tabBar.setLabel(segmentLabel(for: webView, index: index), forSegment: index)
            tabBar.setWidth(80, forSegment: index)
        }
        tabBar.setLabel("+", forSegment: pages.count)
        tabBar.setWidth(pages.isEmpty ? 100 : 80, forSegment: pages.count)
        tabBar.selectedSegment = pages.indices.contains(currentTabIndex) ? currentTabIndex : -1
    }

    private func segmentLabel(for webView: WKWebView, index: Int) -> String {
        let title = webView.title?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        if !title.isEmpty {
            return title
        }
        return "Tab \(index + 1)"
    }

    private func emitTabs() {
        guard let pageId = currentPageId else { return }
        let tabs = zip(pageIds, webViews).map { id, webView in
            ["id": id, "title": webView.title ?? "New Tab",
             "url": webView.url?.absoluteString ?? ""] as [String: Any]
        }
        pageUpdateHandler?(["type": "tabs", "tabs": tabs, "activePageId": pageId])
    }
    
    // MARK: - Navigation
    
    @objc func goBack() {
        guard currentTabIndex < webViews.count else { return }
        let webView = webViews[currentTabIndex]
        if let navigation = webView.goBack() {
            beginNavigation(webView, navigation: navigation)
        } else {
            emitSnapshot(reason: "select")
        }
    }
    
    @objc func goForward() {
        guard currentTabIndex < webViews.count else { return }
        let webView = webViews[currentTabIndex]
        if let navigation = webView.goForward() {
            beginNavigation(webView, navigation: navigation)
        } else {
            emitSnapshot(reason: "select")
        }
    }
    
    @objc func reloadCurrentPage() {
        guard currentTabIndex < webViews.count else { return }
        let webView = webViews[currentTabIndex]
        if let navigation = webView.reload() {
            beginNavigation(webView, navigation: navigation)
        } else {
            emitSnapshot(reason: "select")
        }
    }
    
    @objc private func urlBarAction() {
        let input = urlBar.stringValue.trimmingCharacters(in: .whitespacesAndNewlines)
        navigateCurrentTab(to: input)
    }

    func navigateCurrentTab(to input: String) {
        let input = input.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !input.isEmpty, currentTabIndex < webViews.count else { return }
        guard let url = Self.validatedNavigationURL(from: input) else {
            emitStatus("Only http and https URLs can be opened", for: webViews[currentTabIndex])
            return
        }
        urlBar.stringValue = url.absoluteString
        let webView = webViews[currentTabIndex]
        if let navigation = webView.load(URLRequest(url: url)) {
            beginNavigation(webView, navigation: navigation)
        }
    }

    private func tabPageId(for webView: WKWebView) -> Int? {
        guard let index = webViews.firstIndex(where: { $0 === webView }),
              pageIds.indices.contains(index) else { return nil }
        return pageIds[index]
    }

    private func latestNavigationId(for webView: WKWebView) -> Int {
        latestNavigationIds[ObjectIdentifier(webView)] ?? 0
    }

    private func takeNavigationId(_ navigation: WKNavigation?) -> Int? {
        guard let navigation = navigation else { return nil }
        let id = navigationIds.object(forKey: navigation)?.intValue
        navigationIds.removeObject(forKey: navigation)
        return id
    }

    /// Status for the page in `webView`, stamped with its newest navigation.
    private func emitStatus(_ message: String, for webView: WKWebView) {
        guard let pageId = tabPageId(for: webView) else { return }
        navigationStatuses[ObjectIdentifier(webView)] = message
        pageUpdateHandler?([
            "type": "status",
            "pageId": pageId,
            "navigationId": latestNavigationId(for: webView),
            "message": message
        ])
    }

    /// First-stage http(s) allowlist shared by the URL bar and page WKWebViews.
    /// Scheme-only: does not port Rust netguard DNS/SSRF checks.
    private static func allowsHttpNavigation(_ url: URL) -> Bool {
        guard let scheme = url.scheme?.lowercased() else { return false }
        return scheme == "http" || scheme == "https"
    }

    static func validatedNavigationURL(from input: String) -> URL? {
        let normalized: String
        if input.range(of: #"^[A-Za-z][A-Za-z0-9+.-]*://"#, options: .regularExpression) != nil {
            normalized = input
        } else {
            // A bare hostname may include a port; an explicit non-HTTP scheme
            // must not be reinterpreted as an HTTPS hostname.
            if let scheme = URL(string: input)?.scheme, !scheme.contains(".") {
                return nil
            }
            guard input.contains("."), !input.contains(" ") else { return nil }
            normalized = "https://" + input
        }
        guard let url = URL(string: normalized),
              allowsHttpNavigation(url), url.host != nil else {
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
            "link_count": 0,
            "image_count": 0,
            "form_count": 0,
            "price_count": 0,
            "table_count": 0
        ]
    }

    /// `reason` is "finish" for a completed load and "select" for a tab
    /// switch. A selection snapshot reports whether the page is still loading,
    /// so a slow load is not shown as the previous document being loaded.
    private func emitSnapshot(reason: String, finishedNavigationId: Int? = nil) {
        guard let pageId = currentPageId, currentTabIndex < webViews.count else { return }
        let webView = webViews[currentTabIndex]
        let capturedNavigationId = latestNavigationId(for: webView)
        snapshotCurrentPage { [weak self] snapshot in
            guard let self, self.currentPageId == pageId,
                  self.pageIds.contains(pageId) else { return }
            let latest = self.latestNavigationId(for: webView)
            // A newer navigation started while the page was being read.
            guard capturedNavigationId == latest else { return }
            if let finishedNavigationId, finishedNavigationId != latest { return }
            self.pageUpdateHandler?([
                "type": "snapshot",
                "pageId": pageId,
                "navigationId": latest,
                "reason": reason,
                "loading": finishedNavigationId == nil ? webView.isLoading : false,
                "message": self.navigationStatuses[ObjectIdentifier(webView)] ?? "",
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

    func webView(_ webView: WKWebView, decidePolicyFor navigationAction: WKNavigationAction, decisionHandler: @escaping (WKNavigationActionPolicy) -> Void) {
        guard let url = navigationAction.request.url,
              Self.allowsHttpNavigation(url) else {
            decisionHandler(.cancel)
            let scheme = navigationAction.request.url?.scheme?.lowercased()
            // A fresh webview's about:blank is not a user-facing navigation failure.
            if scheme != "about",
               webViews.indices.contains(currentTabIndex), webViews[currentTabIndex] === webView {
                emitStatus("Only http and https URLs can be opened", for: webView)
            }
            return
        }
        decisionHandler(.allow)
    }
    
    func webView(_ webView: WKWebView, didStartProvisionalNavigation navigation: WKNavigation!) {
        beginNavigation(webView, navigation: navigation)
    }

    private func beginNavigation(_ webView: WKWebView, navigation: WKNavigation?) {
        // Host commands reserve their id immediately: WebKit may cancel the
        // old load before delivering didStart for its replacement.
        if let navigation, navigationIds.object(forKey: navigation) != nil { return }
        guard tabPageId(for: webView) != nil else { return }
        nextNavigationId += 1
        let navigationId = nextNavigationId
        latestNavigationIds[ObjectIdentifier(webView)] = navigationId
        navigationStatuses.removeValue(forKey: ObjectIdentifier(webView))
        if let navigation = navigation {
            navigationIds.setObject(NSNumber(value: navigationId), forKey: navigation)
        }
        if let pageId = tabPageId(for: webView) {
            var event: [String: Any] = ["type": "navigation", "pageId": pageId, "navigationId": navigationId]
            if let url = webView.url?.absoluteString, !url.isEmpty {
                event["url"] = url
            }
            pageUpdateHandler?(event)
        }
        guard webViews.indices.contains(currentTabIndex), webViews[currentTabIndex] === webView else { return }
        reloadButton.title = "◌"
    }

    func webView(_ webView: WKWebView, didReceiveServerRedirectForProvisionalNavigation navigation: WKNavigation!) {
        guard let pageId = tabPageId(for: webView), let url = webView.url?.absoluteString,
              let navigation,
              navigationIds.object(forKey: navigation)?.intValue == latestNavigationId(for: webView) else { return }
        pageUpdateHandler?(["type": "navigation", "pageId": pageId,
                            "navigationId": latestNavigationId(for: webView), "url": url])
        if currentPageId == pageId { urlBar.stringValue = url }
    }

    /// The navigation id for a finished or failed `navigation`, or nil when it
    /// is older than the newest navigation in `webView`.
    private func currentNavigationId(_ navigation: WKNavigation?, in webView: WKWebView) -> Int? {
        let latest = latestNavigationId(for: webView)
        let navigationId: Int
        if navigation != nil {
            guard let knownId = takeNavigationId(navigation) else { return nil }
            navigationId = knownId
        } else {
            navigationId = latest
        }
        return navigationId == latest ? navigationId : nil
    }
    
    func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
        let navigationId = currentNavigationId(navigation, in: webView)
        emitTabs()
        rebuildTabSegments()
        guard webViews.indices.contains(currentTabIndex), webViews[currentTabIndex] === webView,
              let navigationId else { return }
        if let url = webView.url {
            urlBar.stringValue = url.absoluteString
        }
        reloadButton.title = "↻"
        updateNavigationButtons()
        emitSnapshot(reason: "finish", finishedNavigationId: navigationId)
    }
    
    func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: Error) {
        let navigationId = currentNavigationId(navigation, in: webView)
        guard navigationId != nil, tabPageId(for: webView) != nil else { return }
        if webViews.indices.contains(currentTabIndex), webViews[currentTabIndex] === webView {
            reloadButton.title = "↻"
            updateNavigationButtons()
        }
        emitNavigationFailure(error, in: webView)
    }
    
    func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: Error) {
        let navigationId = currentNavigationId(navigation, in: webView)
        guard navigationId != nil, tabPageId(for: webView) != nil else { return }
        if webViews.indices.contains(currentTabIndex), webViews[currentTabIndex] === webView {
            reloadButton.title = "↻"
            updateNavigationButtons()
        }
        emitNavigationFailure(error, in: webView)
    }

    private func emitNavigationFailure(_ error: Error, in webView: WKWebView) {
        let nsError = error as NSError
        // Preserve a policy rejection; an ordinary cancellation must end Loading.
        if nsError.domain == NSURLErrorDomain && nsError.code == NSURLErrorCancelled {
            emitStatus(navigationStatuses[ObjectIdentifier(webView)] ?? "Navigation cancelled", for: webView)
            return
        }
        // WebKitErrorFrameLoadInterruptedByPolicyChange (102) has no WKError.Code case.
        if nsError.domain == "WebKitErrorDomain" && nsError.code == 102 {
            emitStatus(navigationStatuses[ObjectIdentifier(webView)] ?? "Navigation cancelled", for: webView)
            return
        }
        let message = nsError.localizedDescription.isEmpty ? "Page failed to load" : nsError.localizedDescription
        emitStatus(message, for: webView)
    }
}
