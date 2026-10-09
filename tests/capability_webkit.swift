import AppKit
import WebKit

// Executes the shipped page runtime in WebKit; only the host report transport is substituted.
private struct WorkloadFailure: Error, CustomStringConvertible {
    let description: String
    init(_ description: String) { self.description = description }
}

private final class WorkloadHarness: NSObject, WKScriptMessageHandler, WKNavigationDelegate {
    private var reports: [String: [String: Any]] = [:]
    private var requestNumber = 0
    private var navigationFailure: String?
    private var navigationFinished = false
    private let overallDeadline = Date().addingTimeInterval(55)
    private(set) var assertions = 0
    private(set) var evidence: [[String: Any]] = []
    let webview: WKWebView

    init(runtimeScript: String) {
        let controller = WKUserContentController()
        let transport = """
        window.__TAURI_INTERNALS__ = { invoke: (command, args) => {
          if (command !== 'browser_runtime_report') throw new Error('unexpected host command');
          window.webkit.messageHandlers.runtimeReport.postMessage(args.payload);
          return Promise.resolve();
        }};
        """
        controller.addUserScript(WKUserScript(source: transport, injectionTime: .atDocumentStart, forMainFrameOnly: true))
        controller.addUserScript(WKUserScript(source: runtimeScript, injectionTime: .atDocumentStart, forMainFrameOnly: true))
        let configuration = WKWebViewConfiguration()
        configuration.websiteDataStore = .nonPersistent()
        configuration.userContentController = controller
        webview = WKWebView(frame: NSRect(x: 0, y: 0, width: 1024, height: 768), configuration: configuration)
        super.init()
        controller.add(self, name: "runtimeReport")
        webview.navigationDelegate = self
    }

    func userContentController(_ userContentController: WKUserContentController, didReceive message: WKScriptMessage) {
        guard let report = message.body as? [String: Any], let id = report["request_id"] as? String else { return }
        reports[id] = report
    }
    func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
        navigationFinished = true
    }
    func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: Error) {
        navigationFailure = error.localizedDescription
    }
    func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: Error) {
        navigationFailure = error.localizedDescription
    }
    private func wait(_ predicate: () -> Bool, label: String, timeout: TimeInterval = 6) throws {
        let deadline = min(Date().addingTimeInterval(timeout), overallDeadline)
        while !predicate() {
            if let failure = navigationFailure { throw WorkloadFailure("Navigation failed: \(failure)") }
            if Date() >= deadline { throw WorkloadFailure("Timed out waiting for \(label)") }
            RunLoop.current.run(until: Date().addingTimeInterval(0.01))
        }
    }
    func evaluate(_ script: String) throws -> Any {
        var finished = false
        var result: Any?
        var failure: Error?
        webview.evaluateJavaScript(script) { value, error in
            result = value
            failure = error
            finished = true
        }
        try wait({ finished }, label: "JavaScript evaluation")
        if let failure { throw WorkloadFailure("JavaScript evaluation: \(failure.localizedDescription)") }
        return result ?? NSNull()
    }
    func request(_ expression: String) throws -> [String: Any] {
        requestNumber += 1
        let id = "webkit-\(requestNumber)"
        _ = try evaluate("(() => { const runtime = window.__NEUROBROWSER_RUNTIME__; runtime.dispatch(1, '\(id)', () => (\(expression))); })()")
        try wait({ self.reports[id] != nil }, label: "runtime report \(id)")
        let report = reports.removeValue(forKey: id)!
        evidence.append(["request_id": id, "expression": expression, "report": report])
        return report
    }
    func payload(_ expression: String) throws -> [String: Any] {
        let report = try request(expression)
        if let error = report["error"] as? String { throw WorkloadFailure("Runtime rejected \(expression): \(error)") }
        guard let payload = report["payload"] as? [String: Any] else {
            throw WorkloadFailure("Expected object payload from \(expression)")
        }
        return payload
    }
    func check(_ condition: @autoclosure () throws -> Bool, _ message: String) throws {
        if try !condition() { throw WorkloadFailure(message) }
        assertions += 1
    }
    func loadFixture(_ url: URL) throws {
        navigationFinished = false
        navigationFailure = nil
        webview.load(URLRequest(url: url))
        try wait({ self.navigationFinished }, label: "main document HTTP load", timeout: 10)
        try awaitFixture()
    }
    func awaitFixture() throws {
        try wait({ (try? self.evaluate("window.workloadReady === true")) as? Bool == true }, label: "HTTP fact fetch", timeout: 10)
    }
}

private func json(_ value: Any) throws -> String {
    String(data: try JSONSerialization.data(withJSONObject: value, options: [.sortedKeys]), encoding: .utf8)!
}
private let limits: [String: Int] = ["max_text_bytes": 12000, "max_targets": 80, "max_links": 40, "max_tables": 6, "max_rows": 20, "max_cell_bytes": 240]
private func observe(_ harness: WorkloadHarness, _ customLimits: [String: Int] = limits) throws -> [String: Any] {
    try harness.payload("runtime.observe('page-runtime-webkit-workload', \(json(customLimits)))")
}
private func target(_ observation: [String: Any], label: String) throws -> [String: Any] {
    guard let targets = observation["targets"] as? [[String: Any]], let target = targets.first(where: { $0["label"] as? String == label }) else {
        throw WorkloadFailure("Observed target missing: \(label); observed \(observation["targets"] ?? NSNull())")
    }
    return target
}
private func command(_ observation: [String: Any], target: [String: Any], action: String, text: String? = nil) throws -> String {
    guard let document = observation["document"] as? [String: Any], let id = target["id"] as? String else {
        throw WorkloadFailure("Missing observed document or target identity")
    }
    return "runtime.dispatchTarget(\(try json(["document": document, "target_id": id, "action": action, "text": text as Any? ?? NSNull()])))"
}
private func document(_ observation: [String: Any]) throws -> [String: Any] {
    guard let document = observation["document"] as? [String: Any] else { throw WorkloadFailure("Observation has no document stamp") }
    return document
}

@main
private struct CapabilityWebKitWorkloads {
    static func main() {
        let arguments = CommandLine.arguments
        guard arguments.count == 4 else { fputs("Usage: capability-webkit runtime.rs http-url receipt.json\n", stderr); exit(2) }
        var harness: WorkloadHarness?
        var failureMessage: String?
        do {
            let source = try String(contentsOfFile: arguments[1], encoding: .utf8)
            guard let start = source.range(of: "const RUNTIME_INIT_SCRIPT: &str = r#\""),
                  let end = source.range(of: "\"#;", range: start.upperBound..<source.endIndex) else {
                throw WorkloadFailure("Could not extract exact shipped RUNTIME_INIT_SCRIPT")
            }
            let runtimeScript = String(source[start.upperBound..<end.lowerBound])
            NSApplication.shared.setActivationPolicy(.prohibited)
            let test = WorkloadHarness(runtimeScript: runtimeScript)
            harness = test
            try test.loadFixture(URL(string: arguments[2])!)
            try workloads(test)
        } catch { failureMessage = String(describing: error) }
        let receipt: [String: Any] = [
            "schema_version": 1, "engine": "WKWebView", "runtime_source": arguments[1],
            "site": arguments[2], "storage": "nonPersistent", "passed": failureMessage == nil,
            "assertions": harness?.assertions ?? 0, "failure": failureMessage as Any? ?? NSNull(),
            "requests": harness?.evidence ?? [], "collected_at": ISO8601DateFormatter().string(from: Date())
        ]
        do {
            try JSONSerialization.data(withJSONObject: receipt, options: [.prettyPrinted, .sortedKeys])
                .write(to: URL(fileURLWithPath: arguments[3]), options: .atomic)
        } catch { fputs("Could not write WebKit receipt: \(error)\n", stderr); exit(1) }
        if let failureMessage { fputs("WebKit capability workload failed: \(failureMessage)\n", stderr); exit(1) }
        print("Real WebKit capability workloads: \(harness?.assertions ?? 0) assertions passed")
    }

    private static func workloads(_ test: WorkloadHarness) throws {
        let initial = try observe(test)
        let text = initial["text"] as? String ?? ""
        try test.check(text.contains("Depot stock: 17 crates."), "Static fact absent from observation")
        try test.check(text.contains("Next dispatch: North route at 14:30 UTC."), "Fetched JavaScript fact absent from observation")
        try test.check(initial["title"] as? String == "Capability evidence workload", "Native document title was not observed")
        let serialized = try json(initial)
        for canary in ["CANARY_PASSWORD_41", "CANARY_HIDDEN_83", "CANARY_NORMAL_VALUE_97", "CANARY_HIDDEN_TEXT_62"] {
            try test.check(!serialized.contains(canary), "Observation exposed excluded value: \(canary)")
        }
        let tables = initial["tables"] as? [[String: Any]] ?? []
        try test.check(tables.contains(where: { ($0["rows"] as? [[String]])?.contains(["North", "17"]) == true }), "Visible table fact absent")
        let capabilities = initial["capabilities"] as? [String: Any] ?? [:]
        try test.check(capabilities["javascript"] as? Bool == true, "JavaScript capability underreported")
        try test.check(capabilities["scoped_targets"] as? Bool == true, "Scoped targets capability underreported")
        try test.check(capabilities["screenshots"] as? Bool == false, "Unavailable screenshot capability was advertised")
        try test.check(capabilities["enforcing_subresource_network"] as? Bool == false, "Unenforced subresource network capability was advertised")
        let submitTarget = try target(initial, label: "Confirm dispatch")
        try test.check((submitTarget["destination"] as? String ?? "").hasSuffix("/dispatch/reviewed"), "Submit control destination did not retain its formaction override")
        let secret = try target(initial, label: "Access password")
        try test.check(secret["sensitive"] as? Bool == true, "Password target did not retain sensitivity metadata")
        let disabled = try target(initial, label: "Unavailable shipment")
        try test.check(disabled["disabled"] as? Bool == true, "Disabled target metadata absent")
        let observedLabels = (initial["targets"] as? [[String: Any]] ?? []).compactMap { $0["label"] as? String }
        for hiddenLabel in ["CSS dormant control", "Invisible ancestor control", "Inert ancestor control"] {
            try test.check(!observedLabels.contains(hiddenLabel), "Non-rendered target was observed: \(hiddenLabel)")
        }
        let disabledDispatch = try test.payload(command(initial, target: disabled, action: "click"))
        try test.check(disabledDispatch["state"] as? String == "not_dispatched", "Disabled target was dispatched")
        let narrow = try observe(test, ["max_text_bytes": 48, "max_targets": 2, "max_links": 1, "max_tables": 1, "max_rows": 1, "max_cell_bytes": 8])
        try test.check((narrow["text"] as? String ?? "").utf8.count <= 48, "Page text exceeded byte limit")
        try test.check((narrow["targets"] as? [Any] ?? []).count <= 2, "Target ceiling exceeded")
        try test.check((narrow["links"] as? [Any] ?? []).count <= 1, "Link ceiling exceeded")
        let narrowTables = narrow["tables"] as? [[String: Any]] ?? []
        try test.check(narrowTables.count <= 1 && narrowTables.allSatisfy({ ($0["rows"] as? [Any] ?? []).count <= 1 }), "Table ceilings exceeded")

        // A subsequent observation restores the usable target set after the narrow view.
        let beforeClick = try observe(test)
        let click = try test.payload(command(beforeClick, target: target(beforeClick, label: "Advance shipment"), action: "click"))
        try test.check(click["state"] as? String == "acknowledged", "Click was not acknowledged")
        let afterClick = try observe(test)
        try test.check((afterClick["text"] as? String ?? "").contains("Shipment state: dispatched."), "Acknowledged click failed its observed postcondition")
        try test.check(((try document(afterClick)["revision"] as? NSNumber)?.uint64Value ?? 0) > ((try document(beforeClick)["revision"] as? NSNumber)?.uint64Value ?? 0), "DOM mutation did not advance document revision")

        // Replacement reuses a CSS ID intentionally; the original scoped target must be rejected.
        let priorReplacement = try observe(test)
        let originalAdvance = try target(priorReplacement, label: "Advance shipment")
        _ = try test.payload(command(priorReplacement, target: target(priorReplacement, label: "Replace shipment control"), action: "click"))
        let traceBeforeStale = try test.payload("({count: window.workloadTrace.filter(event => event.kind === 'advance').length})")
        let stale = try test.request(command(priorReplacement, target: originalAdvance, action: "click"))
        try test.check((stale["payload"] as? [String: Any])?["state"] as? String == "not_dispatched", "Stale command after button replacement was not rejected")
        let traceAfterStale = try test.payload("({count: window.workloadTrace.filter(event => event.kind === 'advance').length})")
        try test.check(traceBeforeStale["count"] as? Int == traceAfterStale["count"] as? Int, "Rejected stale target still dispatched a page event")
        let afterReplacement = try observe(test)
        let replacement = try target(afterReplacement, label: "Advance replacement shipment")
        try test.check(replacement["id"] as? String != originalAdvance["id"] as? String, "Replacement element reused the prior target identity")

        let recipient = try target(afterReplacement, label: "Recipient")
        // Property writes need no DOM mutation record or input event. The private fingerprint
        // must reject a review that no longer describes the form's current state.
        _ = try test.payload("({set: (document.querySelector('#recipient').value = 'CANARY_PROPERTY_CHANGED_29')})")
        let changedProperty = try test.payload(command(afterReplacement, target: recipient, action: "type", text: "Ada Lovelace"))
        try test.check(changedProperty["state"] as? String == "not_dispatched", "Property-only input mutation bypassed target freshness")
        let freshForm = try observe(test)
        try test.check(!(try json(freshForm)).contains("CANARY_PROPERTY_CHANGED_29"), "Property-only input value escaped observation")
        let typed = try test.payload(command(freshForm, target: target(freshForm, label: "Recipient"), action: "type", text: "Ada Lovelace"))
        try test.check(typed["state"] as? String == "acknowledged", "Typing was not acknowledged")
        let afterType = try observe(test)
        try test.check(!(try json(afterType)).contains("Ada Lovelace"), "Observation exposed an unsubmitted input value")
        let events = try test.payload("({input: window.workloadTrace.some(event => event.kind === 'input' && event.detail === 'Ada Lovelace'), change: window.workloadTrace.some(event => event.kind === 'change' && event.detail === 'Ada Lovelace')})")
        try test.check(events["input"] as? Bool == true && events["change"] as? Bool == true, "Typing failed to emit actual input and change events")
        let submitted = try test.payload(command(afterType, target: target(afterType, label: "Confirm dispatch"), action: "submit"))
        try test.check(submitted["state"] as? String == "acknowledged", "Form submission was not acknowledged")
        let submitter = try test.payload("({event: window.workloadTrace.find(event => event.kind === 'submitter').detail})")
        let submitEvent = submitter["event"] as? [String: Any] ?? [:]
        try test.check(submitEvent["id"] as? String == "confirm-dispatch", "Scoped submission dropped the reviewed submitter")
        try test.check((submitEvent["destination"] as? String ?? "").hasSuffix("/dispatch/reviewed"), "Submission used the default form destination instead of reviewed formaction")
        let afterSubmit = try observe(test)
        try test.check((afterSubmit["text"] as? String ?? "").contains("Form state: dispatched to Ada Lovelace."), "Submitted form failed its observed postcondition")
        let beforeSPA = try observe(test)
        _ = try test.payload(command(beforeSPA, target: target(beforeSPA, label: "Open route view"), action: "click"))
        let afterSPA = try observe(test)
        try test.check((afterSPA["url"] as? String ?? "").hasSuffix("/route/north?view=live"), "SPA navigation URL was not observed")
        try test.check(try document(afterSPA)["document_id"] as? String == document(beforeSPA)["document_id"] as? String, "SPA navigation incorrectly replaced document identity")
        try test.check(((try document(afterSPA)["revision"] as? NSNumber)?.uint64Value ?? 0) > ((try document(beforeSPA)["revision"] as? NSNumber)?.uint64Value ?? 0), "SPA navigation did not advance document revision")
        let staleSPA = try test.request(command(beforeSPA, target: target(beforeSPA, label: "Open route view"), action: "click"))
        try test.check((staleSPA["payload"] as? [String: Any])?["state"] as? String == "not_dispatched", "Command with a stale SPA revision was not rejected")

        var home = URLComponents(url: test.webview.url!, resolvingAgainstBaseURL: false)!
        home.path = "/"; home.query = nil; home.fragment = nil
        try test.loadFixture(home.url!)
        let newDocument = try observe(test)
        try test.check(try document(newDocument)["document_id"] as? String != document(afterSPA)["document_id"] as? String, "Full navigation reused a prior document identity")
        let staleNavigation = try test.payload(command(afterSPA, target: target(afterSPA, label: "Open route view"), action: "click"))
        try test.check(staleNavigation["state"] as? String == "not_dispatched", "Prior document command survived full navigation")
        let finalTrace = try test.payload("({count: window.workloadTrace.filter(event => event.kind === 'spa').length})")
        try test.check(finalTrace["count"] as? Int == 0, "Rejected prior document command dispatched a page event")
    }
}
