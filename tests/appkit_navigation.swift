import AppKit
import WebKit

@main
struct ReloadRegression {
    static func main() {
        let app = NSApplication.shared
        app.setActivationPolicy(.prohibited)
        let controller = ContentViewController()
        _ = controller.view
        controller.addNewTab(pageId: 1)
        let first = controller.webViews[0]
        controller.webView(first, didStartProvisionalNavigation: nil)
        precondition(controller.reloadButton.title == "◌")
        controller.addNewTab(pageId: 2)
        precondition(controller.reloadButton.title == "↻", "switching to an idle tab must clear the spinner")
        let error = NSError(domain: NSURLErrorDomain, code: NSURLErrorCancelled)
        controller.webView(first, didFailProvisionalNavigation: nil, withError: error)
        precondition(controller.reloadButton.title == "↻", "a background failure must preserve the active tab indicator")
        controller.reloadButton.title = "◌"
        controller.webView(first, didFail: nil, withError: error)
        precondition(controller.reloadButton.title == "◌", "a background failure must not clear the active tab indicator")
        let second = controller.webViews[1]
        controller.webView(second, didFail: nil, withError: error)
        precondition(controller.reloadButton.title == "↻", "an active failure must clear the spinner")
        print("AppKit reload indicator: 5/5 assertions passed")
    }
}
