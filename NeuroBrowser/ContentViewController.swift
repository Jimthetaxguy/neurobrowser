import AppKit
import WebKit

class ContentViewController: NSViewController {
    
    // MARK: - UI Components
    
    var urlBar: NSTextField!
    var backButton: NSButton!
    var forwardButton: NSButton!
    var reloadButton: NSButton!
    var tabView: NSTabView!
    var webViews: [WKWebView] = []
    var tabBar: NSSegmentedControl!
    
    // MARK: - State
    
    var currentTabIndex: Int = 0
    
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
        
        // Toolbar container
        let toolbarContainer = NSView()
        toolbarContainer.translatesAutoresizingMaskIntoConstraints = false
        toolbarContainer.wantsLayer = true
        toolbarContainer.layer?.backgroundColor = NSColor.controlBackgroundColor.cgColor
        view.addSubview(toolbarContainer)
        
        // Navigation buttons
        backButton = createNavButton(title: "◀", action: #selector(goBack))
        forwardButton = createNavButton(title: "▶", action: #selector(goForward))
        reloadButton = createNavButton(title: "↻", action: #selector(reloadCurrentPage))
        
        // URL bar
        urlBar = NSTextField()
        urlBar.translatesAutoresizingMaskIntoConstraints = false
        urlBar.placeholderString = "Enter URL or search..."
        urlBar.font = NSFont.systemFont(ofSize: 13)
        urlBar.bezelStyle = .roundedBezel
        urlBar.target = self
        urlBar.action = #selector(urlBarAction)
        toolbarContainer.addSubview(backButton)
        toolbarContainer.addSubview(forwardButton)
        toolbarContainer.addSubview(reloadButton)
        toolbarContainer.addSubview(urlBar)
        
        // Tab bar
        tabBar = NSSegmentedControl()
        tabBar.translatesAutoresizingMaskIntoConstraints = false
        tabBar.segmentCount = 1
        tabBar.setLabel("New Tab", forSegment: 0)
        tabBar.setWidth(100, forSegment: 0)
        tabBar.selectedSegment = 0
        tabBar.target = self
        tabBar.action = #selector(tabBarChanged)
        tabBar.segmentStyle = .rounded
        view.addSubview(tabBar)
        
        // WebView container
        let webViewContainer = NSView()
        webViewContainer.translatesAutoresizingMaskIntoConstraints = false
        view.addSubview(webViewContainer)
        
        // Constraints
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
        
        // Create initial tab with webview
        addNewTab()
    }
    
    private func createNavButton(title: String, action: Selector) -> NSButton {
        let button = NSButton(title: title, target: self, action: action)
        button.translatesAutoresizingMaskIntoConstraints = false
        button.bezelStyle = .rounded
        button.isBordered = true
        return button
    }
    
    // MARK: - Tab Management
    
    func addNewTab() {
        let config = WKWebViewConfiguration()
        let webView = WKWebView(frame: .zero, configuration: config)
        webView.navigationDelegate = self
        
        // Store webview
        webViews.append(webView)
        
        // Update tab bar
        let newIndex = webViews.count - 1
        tabBar.segmentCount = webViews.count + 1
        tabBar.setLabel("Tab \(newIndex + 1)", forSegment: newIndex)
        tabBar.setWidth(80, forSegment: newIndex)
        
        // Add "+" button for new tab
        tabBar.setLabel("+", forSegment: webViews.count)
        
        // Select new tab
        tabBar.selectedSegment = newIndex
        currentTabIndex = newIndex
        
        // Add webview to view hierarchy
        webView.frame = view.bounds
        webView.autoresizingMask = [.width, .height]
        
        // Find webview container
        if let container = view.subviews.last(where: { $0 != tabBar && $0.subviews.isEmpty }) {
            container.addSubview(webView)
        }
        
        // Load default page
        if let url = URL(string: "https://www.example.com") {
            webView.load(URLRequest(url: url))
        }
        
        updateNavigationButtons()
    }
    
    func closeCurrentTab() {
        guard webViews.count > 1 else { return }
        
        let webView = webViews[currentTabIndex]
        webView.removeFromSuperview()
        webViews.remove(at: currentTabIndex)
        
        // Update tab bar
        tabBar.segmentCount = webViews.count + 1
        tabBar.selectedSegment = min(currentTabIndex, webViews.count - 1)
        currentTabIndex = tabBar.selectedSegment
        
        showCurrentTab()
        updateNavigationButtons()
    }
    
    @objc private func tabBarChanged() {
        let selected = tabBar.selectedSegment
        
        // Check if "+" button clicked
        if selected == webViews.count {
            addNewTab()
            return
        }
        
        currentTabIndex = selected
        showCurrentTab()
    }
    
    private func showCurrentTab() {
        // Hide all webviews
        for webView in webViews {
            webView.isHidden = true
        }
        
        // Show current
        if currentTabIndex < webViews.count {
            let webView = webViews[currentTabIndex]
            webView.isHidden = false
            
            // Update URL bar
            if let url = webView.url {
                urlBar.stringValue = url.absoluteString
            }
            
            updateNavigationButtons()
        }
    }
    
    // MARK: - Navigation
    
    @objc private func goBack() {
        guard currentTabIndex < webViews.count else { return }
        webViews[currentTabIndex].goBack()
    }
    
    @objc private func goForward() {
        guard currentTabIndex < webViews.count else { return }
        webViews[currentTabIndex].goForward()
    }
    
    @objc func reloadCurrentPage() {
        guard currentTabIndex < webViews.count else { return }
        webViews[currentTabIndex].reload()
    }
    
    @objc private func urlBarAction() {
        let input = urlBar.stringValue.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !input.isEmpty else { return }
        
        var urlToLoad: URL?
        
        // Check if it looks like a URL
        if input.contains(".") && !input.contains(" ") {
            var urlString = input
            if !urlString.hasPrefix("http://") && !urlString.hasPrefix("https://") {
                urlString = "https://" + urlString
            }
            urlToLoad = URL(string: urlString)
        } else {
            // Search
            let encoded = input.addingPercentEncoding(withAllowedCharacters: .urlQueryAllowed) ?? input
            urlToLoad = URL(string: "https://www.google.com/search?q=\(encoded)")
        }
        
        guard let url = urlToLoad, currentTabIndex < webViews.count else { return }
        webViews[currentTabIndex].load(URLRequest(url: url))
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
        // Show loading
        reloadButton.title = "◌"
    }
    
    func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
        // Update URL bar
        if let url = webView.url {
            urlBar.stringValue = url.absoluteString
        }
        reloadButton.title = "↻"
        updateNavigationButtons()
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
