import AppKit
import WebKit

class BrowserViewController: NSViewController {
    
    // MARK: - UI Components
    
    var splitView: NSSplitViewController!
    var sidebarViewController: SidebarViewController!
    var contentViewController: ContentViewController!
    
    // MARK: - Lifecycle
    
    override func loadView() {
        self.view = NSView(frame: NSRect(x: 0, y: 0, width: 1200, height: 800))
    }
    
    override func viewDidLoad() {
        super.viewDidLoad()
        setupSplitView()
    }
    
    // MARK: - Setup
    
    private func setupSplitView() {
        // Create split view controller
        splitView = NSSplitViewController()
        
        // Sidebar (chat)
        sidebarViewController = SidebarViewController()
        sidebarViewController.controlSurfaceViewController.delegate = self
        let sidebarItem = NSSplitViewItem(sidebarWithViewController: sidebarViewController)
        sidebarItem.canCollapse = true
        sidebarItem.minimumThickness = 300
        sidebarItem.maximumThickness = 440
        splitView.addSplitViewItem(sidebarItem)
        
        // Content (browser)
        contentViewController = ContentViewController()
        contentViewController.pageUpdateHandler = { [weak self] event in
            self?.sidebarViewController.dispatchToReact(event)
        }
        let contentItem = NSSplitViewItem(viewController: contentViewController)
        contentItem.minimumThickness = 400
        splitView.addSplitViewItem(contentItem)
        
        // Add split view as child
        addChild(splitView)
        splitView.view.frame = view.bounds
        splitView.view.autoresizingMask = [.width, .height]
        view.addSubview(splitView.view)
    }
    
    // MARK: - Tab Management
    
    func addNewTab() {
        contentViewController.addNewTab()
    }
    
    func closeCurrentTab() {
        contentViewController.closeCurrentTab()
    }
    
    func toggleSidebar() {
        guard let sidebarItem = splitView.splitViewItems.first else { return }
        sidebarItem.animator().isCollapsed = !sidebarItem.isCollapsed
    }
    
    func reloadCurrentPage() {
        contentViewController.reloadCurrentPage()
    }
}

extension BrowserViewController: ReactControlSurfaceDelegate {
    func controlSurface(_ controlSurface: ReactControlSurfaceViewController, didReceiveCommand command: String, payload: [String: Any]) {
        switch command {
        case "create_session":
            sidebarViewController.dispatchToReact([
                "type": "status",
                "message": "Native AppKit session ready"
            ])
        case "create_page":
            contentViewController.addNewTab()
        case "close_page":
            contentViewController.closeCurrentTab()
        case "set_active_page":
            if let pageId = payload["pageId"] as? Int {
                contentViewController.selectTab(pageId: pageId)
            }
        case "navigate":
            if let url = payload["url"] as? String {
                contentViewController.navigateCurrentTab(to: url)
            }
        case "browser_back":
            contentViewController.goBack()
        case "browser_forward":
            contentViewController.goForward()
        case "browser_reload":
            contentViewController.reloadCurrentPage()
        case "get_page_snapshot":
            contentViewController.snapshotCurrentPage { [weak self] snapshot in
                self?.sidebarViewController.dispatchToReact([
                    "type": "snapshot",
                    "pageId": payload["pageId"] as? Int ?? self?.contentViewController.currentTabIndex ?? 0,
                    "snapshot": snapshot
                ])
            }
        case "set_provider":
            let provider = payload["provider"] as? String ?? "unknown"
            sidebarViewController.dispatchToReact([
                "type": "status",
                "message": "Provider selected in AppKit host: \(provider)"
            ])
        case "ask":
            sidebarViewController.dispatchToReact([
                "type": "status",
                "message": "AppKit command received; Rust agent bridge is pending"
            ])
        default:
            sidebarViewController.dispatchToReact([
                "type": "status",
                "message": "Unhandled native command: \(command)"
            ])
        }
    }
}
