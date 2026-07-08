import AppKit

class SidebarViewController: NSViewController {
    let controlSurfaceViewController = ReactControlSurfaceViewController()

    override func loadView() {
        self.view = NSView(frame: NSRect(x: 0, y: 0, width: 340, height: 800))
    }

    override func viewDidLoad() {
        super.viewDidLoad()
        setupControlSurface()
    }

    func dispatchToReact(_ event: [String: Any]) {
        controlSurfaceViewController.dispatchToReact(event)
    }

    private func setupControlSurface() {
        view.wantsLayer = true
        view.layer?.backgroundColor = NSColor.windowBackgroundColor.cgColor

        addChild(controlSurfaceViewController)
        let controlSurfaceView = controlSurfaceViewController.view
        controlSurfaceView.translatesAutoresizingMaskIntoConstraints = false
        view.addSubview(controlSurfaceView)

        NSLayoutConstraint.activate([
            controlSurfaceView.topAnchor.constraint(equalTo: view.topAnchor),
            controlSurfaceView.leadingAnchor.constraint(equalTo: view.leadingAnchor),
            controlSurfaceView.trailingAnchor.constraint(equalTo: view.trailingAnchor),
            controlSurfaceView.bottomAnchor.constraint(equalTo: view.bottomAnchor)
        ])
    }
}
