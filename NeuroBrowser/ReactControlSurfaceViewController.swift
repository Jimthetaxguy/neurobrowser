import AppKit
import WebKit

protocol ReactControlSurfaceDelegate: AnyObject {
    func controlSurface(_ controlSurface: ReactControlSurfaceViewController, didReceiveCommand command: String, payload: [String: Any])
}

final class ReactControlSurfaceViewController: NSViewController, WKScriptMessageHandler {
    weak var delegate: ReactControlSurfaceDelegate?

    private var webView: WKWebView!
    private var allowedControlDirectory: URL?

    override func loadView() {
        let configuration = WKWebViewConfiguration()
        configuration.processPool = WKProcessPool()
        configuration.websiteDataStore = WKWebsiteDataStore.nonPersistent()
        let userContentController = WKUserContentController()
        userContentController.add(self, name: "neurobrowser")
        configuration.userContentController = userContentController

        webView = WKWebView(frame: .zero, configuration: configuration)
        webView.autoresizingMask = [.width, .height]
        self.view = webView
    }

    override func viewDidLoad() {
        super.viewDidLoad()
        loadControlSurface()
    }

    deinit {
        webView?.configuration.userContentController.removeScriptMessageHandler(forName: "neurobrowser")
    }

    func dispatchToReact(_ event: [String: Any]) {
        guard JSONSerialization.isValidJSONObject(event),
              let data = try? JSONSerialization.data(withJSONObject: event),
              let json = String(data: data, encoding: .utf8) else {
            return
        }

        webView.evaluateJavaScript("window.neurobrowserNativeDispatch && window.neurobrowserNativeDispatch(\(json));")
    }

    private func loadControlSurface() {
        let bundledURL = Bundle.main.url(forResource: "appkit", withExtension: "html", subdirectory: "ControlSurface")
            ?? Bundle.main.url(forResource: "appkit", withExtension: "html")

        if let bundledURL {
            let controlDirectory = bundledURL.deletingLastPathComponent()
            allowedControlDirectory = controlDirectory
            webView.loadFileURL(bundledURL, allowingReadAccessTo: controlDirectory)
            return
        }

        let fallback = """
        <!doctype html>
        <html>
        <body style="font: 13px -apple-system; padding: 16px;">
          <h3>Control surface not built</h3>
          <p>Run npm run build:appkit in src-tauri to generate the AppKit React bundle.</p>
        </body>
        </html>
        """
        webView.loadHTMLString(fallback, baseURL: nil)
    }

    func userContentController(_ userContentController: WKUserContentController, didReceive message: WKScriptMessage) {
        guard message.name == "neurobrowser",
              let body = message.body as? [String: Any],
              let command = body["command"] as? String else {
            return
        }

        let payload = body["payload"] as? [String: Any] ?? [:]
        delegate?.controlSurface(self, didReceiveCommand: command, payload: payload)
    }

    func webView(_ webView: WKWebView, decidePolicyFor navigationAction: WKNavigationAction, decisionHandler: @escaping (WKNavigationActionPolicy) -> Void) {
        decisionHandler(Self.allowsControlFileNavigation(navigationAction.request.url, under: allowedControlDirectory) ? .allow : .cancel)
    }

    /// Bundled control `file://` only. Page http(s) and other schemes stay out of this privileged webview.
    private static func allowsControlFileNavigation(_ url: URL?, under allowedDirectory: URL?) -> Bool {
        guard let url, url.isFileURL, let allowedDirectory else { return false }
        let filePath = url.resolvingSymlinksInPath().path
        let directoryPath = allowedDirectory.resolvingSymlinksInPath().path
        return filePath == directoryPath || filePath.hasPrefix(directoryPath + "/")
    }
}
