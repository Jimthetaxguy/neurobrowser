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
        let second = controller.webViews[1]
        controller.webView(second, didStartProvisionalNavigation: nil)
        let error = NSError(domain: NSURLErrorDomain, code: NSURLErrorCancelled)
        controller.webView(first, didFailProvisionalNavigation: nil, withError: error)
        precondition(controller.reloadButton.title == "◌", "a background provisional failure must preserve the active tab indicator")
        controller.webView(first, didFail: nil, withError: error)
        precondition(controller.reloadButton.title == "◌", "a background failure must not clear the active tab indicator")
        controller.webView(second, didFail: nil, withError: error)
        precondition(controller.reloadButton.title == "↻", "an active failure must clear the spinner")
        var status = ""
        controller.pageUpdateHandler = { event in
            if event["type"] as? String == "status" {
                status = event["message"] as? String ?? ""
            }
        }
        for input in ["file:///blocked.html", "mailto:user@example.com", "javascript:alert(1)", "http:///missing-host"] {
            status = ""
            controller.navigateCurrentTab(to: input)
            precondition(status == "Only http and https URLs can be opened", "a rejected URL must report its reason: \(input)")
        }
        print("AppKit navigation: 9/9 assertions passed")
    }
}
