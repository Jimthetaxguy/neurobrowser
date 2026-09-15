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
        splitView = NSSplitViewController()
        
        sidebarViewController = SidebarViewController()
        sidebarViewController.controlSurfaceViewController.delegate = self
        let sidebarItem = NSSplitViewItem(sidebarWithViewController: sidebarViewController)
        sidebarItem.canCollapse = true
        sidebarItem.minimumThickness = 300
        sidebarItem.maximumThickness = 440
        splitView.addSplitViewItem(sidebarItem)
        
        contentViewController = ContentViewController()
        contentViewController.pageUpdateHandler = { [weak self] event in
            self?.sidebarViewController.dispatchToReact(event)
        }
        let contentItem = NSSplitViewItem(viewController: contentViewController)
        contentItem.minimumThickness = 400
        splitView.addSplitViewItem(contentItem)
        
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
            contentViewController.addNewTab(pageId: Self.payloadPageId(payload))
        case "close_page":
            contentViewController.closePage(pageId: Self.payloadPageId(payload))
        case "set_active_page":
            if let pageId = Self.payloadPageId(payload) {
                contentViewController.selectTab(pageId: pageId)
            }
        case "navigate":
            if let url = payload["url"] as? String {
                contentViewController.navigate(pageId: Self.payloadPageId(payload), to: url)
            }
        case "browser_back":
            contentViewController.goBack()
        case "browser_forward":
            contentViewController.goForward()
        case "browser_reload":
            contentViewController.reloadCurrentPage()
        default:
            sidebarViewController.dispatchToReact([
                "type": "status",
                "message": "Unhandled native command: \(command)"
            ])
        }
    }

    private static func payloadPageId(_ payload: [String: Any]) -> Int? {
        if let value = payload["pageId"] as? Int { return value }
        if let value = payload["pageId"] as? NSNumber { return value.intValue }
        return nil
    }
}
