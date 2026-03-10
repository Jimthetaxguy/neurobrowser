import AppKit

class SidebarViewController: NSViewController {
    
    // MARK: - UI Components
    
    var chatScrollView: NSScrollView!
    var chatTextView: NSTextView!
    var inputTextField: NSTextField!
    var sendButton: NSButton!
    
    // MARK: - Lifecycle
    
    override func loadView() {
        self.view = NSView(frame: NSRect(x: 0, y: 0, width: 300, height: 800))
    }
    
    override func viewDidLoad() {
        super.viewDidLoad()
        setupUI()
    }
    
    // MARK: - Setup
    
    private func setupUI() {
        view.wantsLayer = true
        view.layer?.backgroundColor = NSColor.windowBackgroundColor.cgColor
        
        // Header
        let headerLabel = NSTextField(labelWithString: "AI Assistant")
        headerLabel.font = NSFont.systemFont(ofSize: 14, weight: .semibold)
        headerLabel.textColor = NSColor.labelColor
        headerLabel.translatesAutoresizingMaskIntoConstraints = false
        view.addSubview(headerLabel)
        
        // Chat display area
        chatScrollView = NSScrollView()
        chatScrollView.translatesAutoresizingMaskIntoConstraints = false
        chatScrollView.hasVerticalScroller = true
        chatScrollView.borderType = .noBorder
        chatScrollView.autohidesScrollers = true
        view.addSubview(chatScrollView)
        
        chatTextView = NSTextView()
        chatTextView.isEditable = false
        chatTextView.isSelectable = true
        chatTextView.font = NSFont.systemFont(ofSize: 13)
        chatTextView.textColor = NSColor.labelColor
        chatTextView.backgroundColor = NSColor.textBackgroundColor
        chatTextView.textContainerInset = NSSize(width: 8, height: 8)
        chatScrollView.documentView = chatTextView
        
        // Input area
        let inputContainer = NSView()
        inputContainer.translatesAutoresizingMaskIntoConstraints = false
        inputContainer.wantsLayer = true
        inputContainer.layer?.backgroundColor = NSColor.controlBackgroundColor.cgColor
        view.addSubview(inputContainer)
        
        inputTextField = NSTextField()
        inputTextField.translatesAutoresizingMaskIntoConstraints = false
        inputTextField.placeholderString = "Ask anything..."
        inputTextField.font = NSFont.systemFont(ofSize: 13)
        inputTextField.bezelStyle = .roundedBezel
        inputTextField.target = self
        inputTextField.action = #selector(sendMessage)
        inputContainer.addSubview(inputTextField)
        
        sendButton = NSButton(title: "Send", target: self, action: #selector(sendMessage))
        sendButton.translatesAutoresizingMaskIntoConstraints = false
        sendButton.bezelStyle = .rounded
        sendButton.keyEquivalent = "\r"
        inputContainer.addSubview(sendButton)
        
        // Constraints
        NSLayoutConstraint.activate([
            headerLabel.topAnchor.constraint(equalTo: view.topAnchor, constant: 12),
            headerLabel.leadingAnchor.constraint(equalTo: view.leadingAnchor, constant: 12),
            headerLabel.trailingAnchor.constraint(equalTo: view.trailingAnchor, constant: -12),
            
            chatScrollView.topAnchor.constraint(equalTo: headerLabel.bottomAnchor, constant: 12),
            chatScrollView.leadingAnchor.constraint(equalTo: view.leadingAnchor),
            chatScrollView.trailingAnchor.constraint(equalTo: view.trailingAnchor),
            chatScrollView.bottomAnchor.constraint(equalTo: inputContainer.topAnchor),
            
            inputContainer.leadingAnchor.constraint(equalTo: view.leadingAnchor),
            inputContainer.trailingAnchor.constraint(equalTo: view.trailingAnchor),
            inputContainer.bottomAnchor.constraint(equalTo: view.bottomAnchor),
            inputContainer.heightAnchor.constraint(equalToConstant: 60),
            
            inputTextField.leadingAnchor.constraint(equalTo: inputContainer.leadingAnchor, constant: 8),
            inputTextField.centerYAnchor.constraint(equalTo: inputContainer.centerYAnchor),
            inputTextField.trailingAnchor.constraint(equalTo: sendButton.leadingAnchor, constant: -8),
            
            sendButton.trailingAnchor.constraint(equalTo: inputContainer.trailingAnchor, constant: -8),
            sendButton.centerYAnchor.constraint(equalTo: inputContainer.centerYAnchor),
            sendButton.widthAnchor.constraint(equalToConstant: 60)
        ])
        
        // Welcome message
        appendChatMessage("NeuroBrowser", "Welcome! I'm your AI assistant. Ask me anything or browse the web using the URL bar.")
    }
    
    // MARK: - Actions
    
    @objc private func sendMessage() {
        let message = inputTextField.stringValue.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !message.isEmpty else { return }
        
        // Display user message
        appendChatMessage("You", message)
        inputTextField.stringValue = ""
        
        // TODO: Connect to AI (Loop 9)
        // For now, show a placeholder
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) {
            self.appendChatMessage("NeuroBrowser", "AI integration coming in Loop 9!")
        }
    }
    
    func appendChatMessage(_ sender: String, _ message: String) {
        let currentText = chatTextView.string
        let newText = currentText.isEmpty ? "[\(sender)]\n\(message)" : "\n\n[\(sender)]\n\(message)"
        chatTextView.string = currentText + newText
        
        // Scroll to bottom
        chatTextView.scrollToEndOfDocument(nil)
    }
}
