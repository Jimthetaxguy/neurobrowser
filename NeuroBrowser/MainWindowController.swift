import AppKit
import WebKit

class MainWindowController: NSWindowController {
    
    var browserViewController: BrowserViewController!
    
    convenience init() {
        // Create the main window
        let window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 1200, height: 800),
            styleMask: [.titled, .closable, .miniaturizable, .resizable, .fullSizeContentView],
            backing: .buffered,
            defer: false
        )
        
        window.title = "NeuroBrowser"
        window.minSize = NSSize(width: 800, height: 600)
        window.center()
        window.titlebarAppearsTransparent = false
        window.titleVisibility = .visible
        window.setFrameAutosaveName("MainWindow")
        
        self.init(window: window)
        
        // Setup the browser view controller
        browserViewController = BrowserViewController()
        window.contentViewController = browserViewController
    }
    
    override func windowDidLoad() {
        super.windowDidLoad()
    }
    
    // MARK: - Menu Actions
    
    @objc func newTab(_ sender: Any?) {
        browserViewController.addNewTab()
    }
    
    @objc func closeCurrentTab(_ sender: Any?) {
        browserViewController.closeCurrentTab()
    }
    
    @objc func toggleSidebar(_ sender: Any?) {
        browserViewController.toggleSidebar()
    }
    
    @objc func reloadPage(_ sender: Any?) {
        browserViewController.reloadCurrentPage()
    }
}
