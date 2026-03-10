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
        let sidebarItem = NSSplitViewItem(sidebarWithViewController: sidebarViewController)
        sidebarItem.canCollapse = true
        sidebarItem.minimumThickness = 250
        sidebarItem.maximumThickness = 400
        splitView.addSplitViewItem(sidebarItem)
        
        // Content (browser)
        contentViewController = ContentViewController()
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
