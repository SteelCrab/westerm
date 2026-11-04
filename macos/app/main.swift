// westerm.app: a native macOS window around the westerm dashboard.
//
// - Uses a westerm that already serves on 127.0.0.1:$PORT (default 4317),
//   for example one started in a terminal, so both share the same sessions.
// - Otherwise starts the westerm binary inside the app on that port, or on
//   any free port when another program holds it.
// - Quitting stops only a server the app started. The PTY host keeps
//   running in its own session, so shells survive as they do with the CLI.

import AppKit
import WebKit

final class AppDelegate: NSObject, NSApplicationDelegate, WKNavigationDelegate, WKUIDelegate {
    private let port = Int(ProcessInfo.processInfo.environment["PORT"] ?? "") ?? 4317
    private let logURL = FileManager.default.homeDirectoryForCurrentUser
        .appendingPathComponent("Library/Logs/westerm.log")
    private var window: NSWindow!
    private var webView: WKWebView!
    /// The dashboard server this app started; nil when it uses another one.
    private var server: Process?
    private var dashboard: URL?
    private var serverOutput = ""

    func applicationDidFinishLaunching(_ notification: Notification) {
        NSApp.mainMenu = makeMenu()
        makeWindow()
        showMessage("Starting westerm…")
        NSApp.activate(ignoringOtherApps: true)
        connect()
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        true
    }

    func applicationWillTerminate(_ notification: Notification) {
        // The dashboard server only. The PTY host and its shells live on.
        if let server, server.isRunning {
            server.terminate()
            server.waitUntilExit()
        }
    }

    // MARK: Server

    private func connect() {
        let preferred = URL(string: "http://127.0.0.1:\(port)/")!
        isWesterm(preferred) { [weak self] found in
            guard let self else { return }
            if found {
                self.open(preferred)
                return
            }
            self.startServer(port: self.port) { url in
                if let url {
                    self.open(url)
                    return
                }
                // Another program holds the port; any free port will do.
                self.startServer(port: 0) { url in
                    if let url {
                        self.open(url)
                    } else {
                        self.showMessage("westerm could not start. See \(self.logURL.path).")
                    }
                }
            }
        }
    }

    /// A westerm answers /api/terminal-config with the terminal font settings.
    private func isWesterm(_ base: URL, done: @escaping (Bool) -> Void) {
        var request = URLRequest(url: base.appendingPathComponent("api/terminal-config"))
        request.timeoutInterval = 1
        URLSession.shared.dataTask(with: request) { data, response, _ in
            let json = data.flatMap { try? JSONSerialization.jsonObject(with: $0) as? [String: Any] }
            let found = (response as? HTTPURLResponse)?.statusCode == 200 && json?["fontFamilies"] != nil
            DispatchQueue.main.async { done(found) }
        }.resume()
    }

    /// Runs `westerm serve` from the app bundle and reports the URL it prints,
    /// or nil when it exits first (for example, the port is taken).
    private func startServer(port: Int, done: @escaping (URL?) -> Void) {
        guard let binary = Bundle.main.url(forAuxiliaryExecutable: "westerm") else {
            done(nil)
            return
        }
        let process = Process()
        process.executableURL = binary
        process.arguments = ["serve", "--port", String(port)]
        process.environment = serverEnvironment()
        let pipe = Pipe()
        process.standardOutput = pipe
        process.standardError = pipe

        // Everything below runs on the main queue.
        var reported = false
        let report: (URL?) -> Void = { url in
            guard !reported else { return }
            reported = true
            done(url)
        }
        serverOutput = ""
        pipe.fileHandleForReading.readabilityHandler = { [weak self] handle in
            let data = handle.availableData
            if data.isEmpty {
                handle.readabilityHandler = nil
                return
            }
            DispatchQueue.main.async {
                guard let self else { return }
                self.writeLog(data)
                self.serverOutput += String(decoding: data, as: UTF8.self)
                if let url = Self.dashboardURL(in: self.serverOutput) {
                    report(url)
                }
            }
        }
        process.terminationHandler = { [weak self] _ in
            DispatchQueue.main.async {
                report(nil)
                guard let self, self.server === process else { return }
                self.server = nil
                self.showMessage("westerm stopped. See \(self.logURL.path).")
            }
        }
        do {
            try process.run()
            server = process
        } catch {
            writeLog(Data("could not run \(binary.path): \(error)\n".utf8))
            report(nil)
        }
    }

    /// The URL in "> westerm: http://127.0.0.1:4317", once its line is complete.
    static func dashboardURL(in text: String) -> URL? {
        guard let marker = text.range(of: "westerm: http://127.0.0.1:") else { return nil }
        let rest = text[marker.upperBound...]
        let digits = rest.prefix(while: \.isNumber)
        guard !digits.isEmpty, rest.count > digits.count else { return nil }
        return URL(string: "http://127.0.0.1:\(digits)/")
    }

    /// Apps opened from Finder get no LANG. Shells then fall back to the C
    /// locale and break Korean, Japanese, and Chinese text, so set a UTF-8
    /// locale from the system language when none is given.
    private func serverEnvironment() -> [String: String] {
        var environment = ProcessInfo.processInfo.environment
        if environment["LANG"] == nil && environment["LC_ALL"] == nil && environment["LC_CTYPE"] == nil {
            environment["LANG"] = Self.utf8Locale()
        }
        return environment
    }

    static func utf8Locale() -> String {
        let locale = NSLocale.current as NSLocale
        if let language = locale.languageCode as String?, let region = locale.countryCode {
            let name = "\(language)_\(region).UTF-8"
            if FileManager.default.fileExists(atPath: "/usr/share/locale/\(name)") {
                return name
            }
        }
        return "en_US.UTF-8"
    }

    private func writeLog(_ data: Data) {
        let manager = FileManager.default
        try? manager.createDirectory(at: logURL.deletingLastPathComponent(), withIntermediateDirectories: true)
        if !manager.fileExists(atPath: logURL.path) {
            manager.createFile(atPath: logURL.path, contents: nil)
        }
        guard let handle = try? FileHandle(forWritingTo: logURL) else { return }
        handle.seekToEndOfFile()
        handle.write(data)
        try? handle.close()
    }

    // MARK: Window

    private func makeWindow() {
        let configuration = WKWebViewConfiguration()
        webView = WKWebView(frame: .zero, configuration: configuration)
        webView.navigationDelegate = self
        webView.uiDelegate = self
        if #available(macOS 13.3, *) {
            // Safari ▸ Develop can inspect the dashboard.
            webView.isInspectable = true
        }
        window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 1280, height: 820),
            styleMask: [.titled, .closable, .miniaturizable, .resizable],
            backing: .buffered,
            defer: false
        )
        window.title = "westerm"
        window.minSize = NSSize(width: 640, height: 400)
        window.tabbingMode = .disallowed
        window.contentView = webView
        window.center()
        window.setFrameAutosaveName("westerm main window")
        window.makeKeyAndOrderFront(nil)
    }

    private func open(_ url: URL) {
        dashboard = url
        webView.load(URLRequest(url: url))
    }

    private func showMessage(_ text: String) {
        let escaped = text
            .replacingOccurrences(of: "&", with: "&amp;")
            .replacingOccurrences(of: "<", with: "&lt;")
        let html = """
        <!doctype html><meta charset="utf-8">
        <style>
          :root { color-scheme: light dark; }
          body { margin: 0; height: 100vh; display: grid; place-items: center;
                 font: 14px -apple-system, sans-serif; color: #8b8f98; background: #f6f7f9; }
          @media (prefers-color-scheme: dark) { body { background: #16181d; } }
        </style>
        <p>\(escaped)</p>
        """
        webView.loadHTMLString(html, baseURL: nil)
    }

    // MARK: Navigation

    /// The dashboard stays in the window; any other page opens in the browser.
    func webView(
        _ webView: WKWebView,
        decidePolicyFor navigationAction: WKNavigationAction,
        decisionHandler: @escaping (WKNavigationActionPolicy) -> Void
    ) {
        guard let url = navigationAction.request.url, url.scheme == "http" || url.scheme == "https" else {
            decisionHandler(.allow)
            return
        }
        if url.host == "127.0.0.1", url.port == dashboard?.port {
            decisionHandler(.allow)
        } else {
            NSWorkspace.shared.open(url)
            decisionHandler(.cancel)
        }
    }

    func webView(
        _ webView: WKWebView,
        createWebViewWith configuration: WKWebViewConfiguration,
        for navigationAction: WKNavigationAction,
        windowFeatures: WKWindowFeatures
    ) -> WKWebView? {
        if let url = navigationAction.request.url {
            NSWorkspace.shared.open(url)
        }
        return nil
    }

    func webViewWebContentProcessDidTerminate(_ webView: WKWebView) {
        // Panes reattach to their sessions after a reload.
        if let dashboard { webView.load(URLRequest(url: dashboard)) }
    }

    // MARK: Menu

    @objc private func reload(_ sender: Any?) {
        if let dashboard { webView.load(URLRequest(url: dashboard)) }
    }

    @objc private func openInBrowser(_ sender: Any?) {
        if let dashboard { NSWorkspace.shared.open(dashboard) }
    }

    private func makeMenu() -> NSMenu {
        let main = NSMenu()

        let app = NSMenu()
        app.addItem(withTitle: "About westerm", action: #selector(NSApplication.orderFrontStandardAboutPanel(_:)), keyEquivalent: "")
        app.addItem(.separator())
        app.addItem(withTitle: "Hide westerm", action: #selector(NSApplication.hide(_:)), keyEquivalent: "h")
        app.addItem(withTitle: "Hide Others", action: #selector(NSApplication.hideOtherApplications(_:)), keyEquivalent: "h")
            .keyEquivalentModifierMask = [.command, .option]
        app.addItem(withTitle: "Show All", action: #selector(NSApplication.unhideAllApplications(_:)), keyEquivalent: "")
        app.addItem(.separator())
        app.addItem(withTitle: "Quit westerm", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
        main.addItem(submenu(app, title: "westerm"))

        // Copy and paste reach the terminal through the web view's own handlers.
        let edit = NSMenu(title: "Edit")
        edit.addItem(withTitle: "Undo", action: Selector(("undo:")), keyEquivalent: "z")
        edit.addItem(withTitle: "Redo", action: Selector(("redo:")), keyEquivalent: "z")
            .keyEquivalentModifierMask = [.command, .shift]
        edit.addItem(.separator())
        edit.addItem(withTitle: "Cut", action: #selector(NSText.cut(_:)), keyEquivalent: "x")
        edit.addItem(withTitle: "Copy", action: #selector(NSText.copy(_:)), keyEquivalent: "c")
        edit.addItem(withTitle: "Paste", action: #selector(NSText.paste(_:)), keyEquivalent: "v")
        edit.addItem(withTitle: "Select All", action: #selector(NSText.selectAll(_:)), keyEquivalent: "a")
        main.addItem(submenu(edit, title: "Edit"))

        let view = NSMenu(title: "View")
        view.addItem(withTitle: "Reload", action: #selector(reload(_:)), keyEquivalent: "r").target = self
        view.addItem(withTitle: "Open in Browser", action: #selector(openInBrowser(_:)), keyEquivalent: "").target = self
        view.addItem(.separator())
        view.addItem(withTitle: "Enter Full Screen", action: #selector(NSWindow.toggleFullScreen(_:)), keyEquivalent: "f")
            .keyEquivalentModifierMask = [.command, .control]
        main.addItem(submenu(view, title: "View"))

        let windowMenu = NSMenu(title: "Window")
        windowMenu.addItem(withTitle: "Minimize", action: #selector(NSWindow.performMiniaturize(_:)), keyEquivalent: "m")
        windowMenu.addItem(withTitle: "Zoom", action: #selector(NSWindow.performZoom(_:)), keyEquivalent: "")
        windowMenu.addItem(withTitle: "Close", action: #selector(NSWindow.performClose(_:)), keyEquivalent: "w")
        main.addItem(submenu(windowMenu, title: "Window"))
        NSApp.windowsMenu = windowMenu

        return main
    }

    private func submenu(_ menu: NSMenu, title: String) -> NSMenuItem {
        let item = NSMenuItem(title: title, action: nil, keyEquivalent: "")
        item.submenu = menu
        return item
    }
}

let application = NSApplication.shared
let delegate = AppDelegate()
application.delegate = delegate
application.setActivationPolicy(.regular)
application.run()
