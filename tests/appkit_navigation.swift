import AppKit
import WebKit

private final class NavigationProbe: ContentViewController {
    var starts = 0
    override func webView(_ webView: WKWebView, didStartProvisionalNavigation navigation: WKNavigation!) {
        starts += 1
        super.webView(webView, didStartProvisionalNavigation: navigation)
    }
}

@main
struct ReloadRegression {
    static func realNavigation(base: String) {
        let controller = NavigationProbe()
        _ = controller.view
        controller.addNewTab(pageId: 10)
        let page = controller.webViews[0]
        var events: [[String: Any]] = []
        controller.pageUpdateHandler = { events.append($0) }
        func wait(_ label: String, _ predicate: () -> Bool) {
            let deadline = Date().addingTimeInterval(10)
            while !predicate() {
                precondition(Date() < deadline, "Timed out: \(label); events: \(events)")
                RunLoop.current.run(until: Date().addingTimeInterval(0.01))
            }
        }
        func finished(_ path: String) -> Bool {
            events.contains { event in
                let snapshot = event["snapshot"] as? [String: Any]
                return event["reason"] as? String == "finish" && snapshot?["url"] as? String == base + path
            }
        }
        func load(_ path: String) {
            events.removeAll()
            controller.navigateCurrentTab(to: base + path)
            wait("finish \(path)") { finished(path) }
            precondition(controller.reloadButton.title == "↻")
        }
        // An idle history command must resolve the panel's pending status.
        events.removeAll()
        controller.goBack()
        wait("empty history") { events.contains { $0["reason"] as? String == "select" } }
        load("/first")
        events.removeAll()
        controller.navigateCurrentTab(to: base + "/redirect")
        wait("redirect") { finished("/final") }
        precondition(controller.urlBar.stringValue == base + "/final")
        precondition(events.contains { $0["type"] as? String == "navigation" && $0["url"] as? String == base + "/final" })
        events.removeAll()
        controller.goBack()
        wait("back") { finished("/first") }
        events.removeAll()
        controller.goForward()
        wait("forward") { finished("/final") }
        events.removeAll()
        controller.reloadCurrentPage()
        wait("reload") { finished("/final") }
        load("/first")
        events.removeAll()
        controller.navigateCurrentTab(to: base + "/failure")
        wait("failure") { events.contains { $0["type"] as? String == "status" } }
        precondition(controller.reloadButton.title == "↻")
        load("/recovered")
        events.removeAll()
        let beforeCancel = controller.starts
        controller.navigateCurrentTab(to: base + "/slow")
        wait("slow WebKit start") { controller.starts > beforeCancel }
        page.stopLoading()
        wait("cancel") { events.contains { $0["message"] as? String == "Navigation cancelled" } }
        precondition(controller.reloadButton.title == "↻")
        // Switching away and back must retain the cancellation, not show Loaded.
        controller.addNewTab(pageId: 11)
        events.removeAll()
        controller.selectTab(pageId: 10)
        wait("cancel selection") { events.contains { $0["reason"] as? String == "select" && $0["message"] as? String == "Navigation cancelled" } }
        events.removeAll()
        let beforeReplacement = controller.starts
        controller.navigateCurrentTab(to: base + "/slow")
        wait("replacement WebKit start") { controller.starts > beforeReplacement }
        events.removeAll()
        controller.navigateCurrentTab(to: base + "/replacement")
        wait("replacement finish") { finished("/replacement") }
        precondition(!events.contains { $0["type"] as? String == "status" }, "old cancellation must not fail the replacement: \(events)")
        // A selection read already in flight cannot be relabeled with a new load id.
        events.removeAll()
        controller.selectTab(pageId: 11)
        controller.selectTab(pageId: 10)
        controller.navigateCurrentTab(to: base + "/last")
        wait("last finish") { finished("/last") }
        let starts = events.filter { $0["type"] as? String == "navigation" }
        let latest = starts.last?["navigationId"] as? Int
        precondition(events.filter { $0["reason"] as? String == "finish" }.allSatisfy { $0["navigationId"] as? Int == latest })
        precondition(!events.contains { event in
            let snapshot = event["snapshot"] as? [String: Any]
            return event["reason"] as? String == "select" && event["navigationId"] as? Int == latest
                && snapshot?["url"] as? String != base + "/last"
        }, "a stale selection must not borrow the replacement's navigation id")
        // Several host requests in one run-loop turn must settle on the last.
        events.removeAll()
        for index in 0..<8 { controller.navigateCurrentTab(to: base + "/rapid-\(index)") }
        wait("rapid navigation") { finished("/rapid-7") }
        precondition(controller.urlBar.stringValue == base + "/rapid-7")
        precondition(controller.reloadButton.title == "↻")
        // A hidden page's network failure must be restored when selected.
        events.removeAll()
        controller.navigateCurrentTab(to: base + "/failure")
        controller.selectTab(pageId: 11)
        wait("background failure") { events.contains { $0["type"] as? String == "status" && $0["pageId"] as? Int == 10 } }
        events.removeAll()
        controller.selectTab(pageId: 10)
        wait("failure selection") { events.contains { $0["reason"] as? String == "select" && !($0["message"] as? String ?? "").isEmpty } }
    }

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
        for input in ["file:///blocked.html", "mailto:user@example.com", "javascript:alert(1)", "http:///missing-host", "a.b://blocked.example"] {
            status = ""
            controller.navigateCurrentTab(to: input)
            precondition(status == "Only http and https URLs can be opened", "a rejected URL must report its reason: \(input)")
        }
        let accepted = [
            ("example.com/?next=https://other.example", "https://example.com/?next=https://other.example"),
            ("example.com/path/https://other.example", "https://example.com/path/https://other.example"),
            ("example.com:8443/?next=https://other.example", "https://example.com:8443/?next=https://other.example"),
            ("HTTPS://example.com/", "HTTPS://example.com/"),
            ("example.com", "https://example.com")
        ]
        for (input, expected) in accepted {
            precondition(ContentViewController.validatedNavigationURL(from: input)?.absoluteString == expected,
                         "a valid URL must retain its path and query: \(input)")
        }
        // Events carry the page and WebKit navigation id, for every tab.
        var events: [[String: Any]] = []
        controller.pageUpdateHandler = { event in events.append(event) }
        controller.webView(first, didStartProvisionalNavigation: nil)
        let backgroundStart = events.last
        precondition(backgroundStart?["type"] as? String == "navigation"
                     && backgroundStart?["pageId"] as? Int == 1,
                     "a background tab's navigation start must name its page")
        let backgroundId = backgroundStart?["navigationId"] as? Int ?? 0
        controller.webView(second, didStartProvisionalNavigation: nil)
        let activeId = events.last?["navigationId"] as? Int ?? 0
        precondition(events.last?["pageId"] as? Int == 2 && activeId > backgroundId,
                     "each navigation start must get a newer id")
        events.removeAll()
        controller.navigateCurrentTab(to: "file:///blocked.html")
        precondition(events.count == 1 && events[0]["type"] as? String == "status"
                     && events[0]["pageId"] as? Int == 2 && events[0]["navigationId"] as? Int == activeId,
                     "a rejection must name the active page and its newest navigation")
        events.removeAll()
        precondition(controller.selectTab(pageId: 2))
        precondition(events.isEmpty, "a command for the shown tab is not a tab switch")
        precondition(controller.selectTab(pageId: 1))
        precondition(events.contains { $0["type"] as? String == "tabs" && $0["activePageId"] as? Int == 1 },
                     "a tab switch must report the newly active page")
        realNavigation(base: CommandLine.arguments[1])
        print("AppKit navigation: delegate regressions and real WebKit navigation passed")
    }
}
