// westerm.app: a native macOS window around the westerm dashboard.
//
// - Uses a westerm that already serves on 127.0.0.1:$PORT (default 4317),
//   for example one started in a terminal, so both share the same sessions.
// - Otherwise starts the westerm binary inside the app on that port, or on
//   any free port when another program holds it.
// - Quitting stops only a server the app started. The PTY host keeps
//   running in its own session, so shells survive as they do with the CLI.
// - Check for Updates replaces the whole app (Updater.swift); the page's
//   update button asks for it through the "westerm" message handler.

import AppKit
import WebKit

final class AppDelegate: NSObject, NSApplicationDelegate, NSWindowDelegate, WKNavigationDelegate, WKUIDelegate, WKScriptMessageHandler {
    private let port = Int(ProcessInfo.processInfo.environment["PORT"] ?? "") ?? 4317
    private let logURL = FileManager.default.homeDirectoryForCurrentUser
        .appendingPathComponent("Library/Logs/westerm.log")
    private var window: NSWindow!
    private var webView: WKWebView!
    /// One window per remote server, by target. The page opens them with
    /// window.open("/?remote=<target>"); see createWebViewWith.
    private var remoteWindows: [String: NSWindow] = [:]
    /// Keeps each remote window's title in step with its page title.
    private var remoteTitles: [String: NSKeyValueObservation] = [:]
    /// The dashboard server this app started; nil when it uses another one.
    private var server: Process?
    private var dashboard: URL?
    private var serverOutput = ""
    private var updating = false

    func applicationDidFinishLaunching(_ notification: Notification) {
        NSApp.mainMenu = makeMenu()
        makeWindow()
        showMessage(L.starting)
        NSApp.activate(ignoringOtherApps: true)
        connect()
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        true
    }

    /// A click on the Dock icon brings the main window back when it is closed
    /// or minimized, even while remote windows are open.
    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool) -> Bool {
        if !window.isVisible { raise(window) }
        return true
    }

    private func raise(_ target: NSWindow) {
        if target.isMiniaturized { target.deminiaturize(nil) }
        target.makeKeyAndOrderFront(nil)
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
                        self.showMessage(L.cannotStart(self.logURL.path))
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
                self.showMessage(L.stopped(self.logURL.path))
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
        // The page's update button posts {type: "update"} here.
        configuration.userContentController.add(self, name: "westerm")
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
        window.isReleasedWhenClosed = false
        window.contentView = webView
        window.center()
        window.setFrameAutosaveName("westerm main window")
        window.makeKeyAndOrderFront(nil)
    }

    private func open(_ url: URL) {
        dashboard = url
        webView.load(URLRequest(url: url))
    }

    /// The target of a `/?remote=<target>` page on the dashboard's own origin,
    /// or nil for any other URL.
    private func remoteTarget(_ url: URL) -> String? {
        guard url.scheme == "http", url.host == "127.0.0.1", url.port == dashboard?.port else { return nil }
        let items = URLComponents(url: url, resolvingAgainstBaseURL: false)?.queryItems
        guard let target = items?.first(where: { $0.name == "remote" })?.value, !target.isEmpty else { return nil }
        return target
    }

    /// Puts the web view WebKit made for window.open into a new window.
    private func makeRemoteWindow(_ target: String, configuration: WKWebViewConfiguration) -> WKWebView {
        let remoteView = WKWebView(frame: .zero, configuration: configuration)
        remoteView.navigationDelegate = self
        remoteView.uiDelegate = self
        if #available(macOS 13.3, *) {
            remoteView.isInspectable = true
        }
        let remote = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 1280, height: 820),
            styleMask: [.titled, .closable, .miniaturizable, .resizable],
            backing: .buffered,
            defer: false
        )
        remote.title = "westerm — \(target)"
        remote.minSize = NSSize(width: 640, height: 400)
        remote.tabbingMode = .disallowed
        remote.isReleasedWhenClosed = false
        remote.delegate = self
        remote.contentView = remoteView
        remote.center()
        remote.setFrameAutosaveName("westerm remote \(target)")
        remoteWindows[target] = remote
        remoteTitles[target] = remoteView.observe(\.title) { [weak remote] view, _ in
            if let title = view.title, !title.isEmpty { remote?.title = title }
        }
        remote.makeKeyAndOrderFront(nil)
        return remoteView
    }

    func windowWillClose(_ notification: Notification) {
        guard let closed = notification.object as? NSWindow,
              let target = remoteWindows.first(where: { $0.value === closed })?.key else { return }
        remoteWindows[target] = nil
        remoteTitles[target] = nil
        closed.delegate = nil
    }

    /// The web view of the key window; the main one when no window is key.
    private var activeWebView: WKWebView {
        NSApp.keyWindow?.contentView as? WKWebView ?? webView
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
        guard let url = navigationAction.request.url else { return nil }
        guard let target = remoteTarget(url) else {
            NSWorkspace.shared.open(url)
            return nil
        }
        if let existing = remoteWindows[target] {
            raise(existing)
            return nil
        }
        return makeRemoteWindow(target, configuration: configuration)
    }

    /// The page asks for a remote window through the message handler: a page
    /// cannot raise a window it opened, so the app does it on every request.
    private func openRemoteWindow(_ target: String) {
        if let existing = remoteWindows[target] {
            raise(existing)
            return
        }
        guard let dashboard, var components = URLComponents(url: dashboard, resolvingAgainstBaseURL: false) else { return }
        // Encode everything but unreserved characters, so "+" cannot turn into a space.
        let unreserved = CharacterSet(charactersIn: "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~")
        guard let value = target.addingPercentEncoding(withAllowedCharacters: unreserved) else { return }
        components.path = "/"
        components.percentEncodedQuery = "remote=\(value)"
        guard let url = components.url, remoteTarget(url) == target else { return }
        // The main configuration carries the "westerm" message handler.
        makeRemoteWindow(target, configuration: webView.configuration).load(URLRequest(url: url))
    }

    func webViewWebContentProcessDidTerminate(_ webView: WKWebView) {
        // Panes reattach to their sessions after a reload. Each window
        // reloads its own page; the main one goes back to the dashboard.
        if webView === self.webView {
            if let dashboard { webView.load(URLRequest(url: dashboard)) }
        } else if let url = webView.url {
            webView.load(URLRequest(url: url))
        } else {
            webView.reload()
        }
    }

    func userContentController(_ controller: WKUserContentController, didReceive message: WKScriptMessage) {
        guard let body = message.body as? [String: Any] else { return }
        switch body["type"] as? String {
        case "update":
            checkForUpdates(userInitiated: true)
        case "remoteWindow":
            if let target = body["target"] as? String, !target.isEmpty { openRemoteWindow(target) }
        default:
            break
        }
    }

    // MARK: Updates

    private var appVersion: String {
        Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "0.0.0"
    }

    @objc private func checkForUpdates(_ sender: Any?) {
        checkForUpdates(userInitiated: true)
    }

    private func checkForUpdates(userInitiated: Bool) {
        guard !updating else { return }
        Updater.latest { release, error in
            DispatchQueue.main.async {
                guard let release else {
                    if userInitiated { self.alert(L.checkFailed, error?.localizedDescription ?? "") }
                    return
                }
                if Updater.isNewer(release.version, than: self.appVersion) {
                    self.offer(release)
                } else if userInitiated {
                    self.alert(L.upToDate(self.appVersion), "")
                }
            }
        }
    }

    private func offer(_ release: AppRelease) {
        let alert = NSAlert()
        alert.messageText = L.available(release.version)
        alert.informativeText = L.availableDetail(appVersion)
        alert.addButton(withTitle: L.install)
        alert.addButton(withTitle: L.later)
        if release.page != nil { alert.addButton(withTitle: L.releaseNotes) }
        switch alert.runModal() {
        case .alertFirstButtonReturn: install(release)
        case .alertThirdButtonReturn: if let page = release.page { NSWorkspace.shared.open(page) }
        default: break
        }
    }

    /// Downloads and checks the new app off the main thread, then swaps the
    /// bundle and opens the new copy.
    private func install(_ release: AppRelease) {
        let bundle = Bundle.main.bundleURL
        let folder = bundle.deletingLastPathComponent().path
        // A downloaded app opened in place runs from a read-only copy, and one
        // opened from the disk image runs from a read-only volume.
        let readOnlyVolume = (try? bundle.resourceValues(forKeys: [.volumeIsReadOnlyKey]))?.volumeIsReadOnly ?? false
        if bundle.path.contains("/AppTranslocation/") || readOnlyVolume {
            alert(L.updateFailed, L.moveToApplications)
            return
        }
        guard FileManager.default.isWritableFile(atPath: folder) else {
            alert(L.updateFailed, L.cannotWrite(folder))
            return
        }
        updating = true
        window.title = "westerm — \(L.updating)"
        DispatchQueue.global(qos: .userInitiated).async {
            let prepared = Result { try Updater.prepare(release) }
            DispatchQueue.main.async {
                self.updating = false
                self.window.title = "westerm"
                switch prepared {
                case .failure(let error): self.alert(L.updateFailed, error.localizedDescription)
                case .success(let newApp): self.relaunch(into: newApp, at: bundle)
                }
            }
        }
    }

    private func relaunch(into newApp: URL, at bundle: URL) {
        // Stop the dashboard this app started, so the new copy starts its own
        // instead of finding this one.
        if let server, server.isRunning {
            self.server = nil
            server.terminate()
            server.waitUntilExit()
        }
        do {
            try Updater.swap(newApp, into: bundle)
        } catch {
            alert(L.updateFailed, error.localizedDescription)
            return
        }
        let configuration = NSWorkspace.OpenConfiguration()
        configuration.createsNewApplicationInstance = true
        NSWorkspace.shared.openApplication(at: bundle, configuration: configuration) { _, error in
            DispatchQueue.main.async {
                if let error {
                    self.alert(L.updateFailed, error.localizedDescription)
                } else {
                    NSApp.terminate(nil)
                }
            }
        }
    }

    private func alert(_ title: String, _ detail: String) {
        let alert = NSAlert()
        alert.messageText = title
        alert.informativeText = detail
        alert.runModal()
    }

    // MARK: Menu

    /// Tab and pane switching. The page handles these keys itself (see
    /// src/shortcuts.ts); the menu items show them and work by click.
    @objc private func sendSwitch(_ sender: NSMenuItem) {
        guard let command = sender.representedObject as? String else { return }
        activeWebView.evaluateJavaScript("window.dispatchEvent(new CustomEvent('westerm:command', { detail: \(command) }))")
    }

    private func switchItem(_ title: String, _ command: String, key: String, _ modifiers: NSEvent.ModifierFlags) -> NSMenuItem {
        let item = NSMenuItem(title: title, action: #selector(sendSwitch(_:)), keyEquivalent: key)
        item.keyEquivalentModifierMask = modifiers
        item.representedObject = command
        item.target = self
        return item
    }

    /// The page opens its settings when it hears this event.
    @objc private func openSettings(_ sender: Any?) {
        activeWebView.evaluateJavaScript("window.dispatchEvent(new Event('westerm:settings'))")
    }

    @objc private func reload(_ sender: Any?) {
        let view = activeWebView
        if view === webView {
            if let dashboard { view.load(URLRequest(url: dashboard)) }
        } else {
            view.reload()
        }
    }

    @objc private func openInBrowser(_ sender: Any?) {
        let view = activeWebView
        if let url = view === webView ? dashboard : view.url { NSWorkspace.shared.open(url) }
    }

    private func makeMenu() -> NSMenu {
        let main = NSMenu()

        let app = NSMenu()
        app.addItem(withTitle: L.about, action: #selector(NSApplication.orderFrontStandardAboutPanel(_:)), keyEquivalent: "")
        app.addItem(withTitle: L.checkForUpdates, action: #selector(checkForUpdates(_:)), keyEquivalent: "").target = self
        app.addItem(.separator())
        app.addItem(withTitle: L.settings, action: #selector(openSettings(_:)), keyEquivalent: ",").target = self
        app.addItem(.separator())
        app.addItem(withTitle: L.hide, action: #selector(NSApplication.hide(_:)), keyEquivalent: "h")
        app.addItem(withTitle: L.hideOthers, action: #selector(NSApplication.hideOtherApplications(_:)), keyEquivalent: "h")
            .keyEquivalentModifierMask = [.command, .option]
        app.addItem(withTitle: L.showAll, action: #selector(NSApplication.unhideAllApplications(_:)), keyEquivalent: "")
        app.addItem(.separator())
        app.addItem(withTitle: L.quit, action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
        main.addItem(submenu(app, title: "westerm"))

        // Copy and paste reach the terminal through the web view's own handlers.
        let edit = NSMenu(title: L.edit)
        edit.addItem(withTitle: L.undo, action: Selector(("undo:")), keyEquivalent: "z")
        edit.addItem(withTitle: L.redo, action: Selector(("redo:")), keyEquivalent: "z")
            .keyEquivalentModifierMask = [.command, .shift]
        edit.addItem(.separator())
        edit.addItem(withTitle: L.cut, action: #selector(NSText.cut(_:)), keyEquivalent: "x")
        edit.addItem(withTitle: L.copy, action: #selector(NSText.copy(_:)), keyEquivalent: "c")
        edit.addItem(withTitle: L.paste, action: #selector(NSText.paste(_:)), keyEquivalent: "v")
        edit.addItem(withTitle: L.selectAll, action: #selector(NSText.selectAll(_:)), keyEquivalent: "a")
        main.addItem(submenu(edit, title: L.edit))

        let view = NSMenu(title: L.view)
        view.addItem(withTitle: L.reload, action: #selector(reload(_:)), keyEquivalent: "r").target = self
        view.addItem(withTitle: L.openInBrowser, action: #selector(openInBrowser(_:)), keyEquivalent: "").target = self
        view.addItem(.separator())
        view.addItem(withTitle: L.fullScreen, action: #selector(NSWindow.toggleFullScreen(_:)), keyEquivalent: "f")
            .keyEquivalentModifierMask = [.command, .control]
        main.addItem(submenu(view, title: L.view))

        let windowMenu = NSMenu(title: L.window)
        windowMenu.addItem(withTitle: L.minimize, action: #selector(NSWindow.performMiniaturize(_:)), keyEquivalent: "m")
        windowMenu.addItem(withTitle: L.zoom, action: #selector(NSWindow.performZoom(_:)), keyEquivalent: "")
        windowMenu.addItem(withTitle: L.close, action: #selector(NSWindow.performClose(_:)), keyEquivalent: "w")
        windowMenu.addItem(.separator())
        windowMenu.addItem(switchItem(L.nextTab, #"{"type":"tabStep","delta":1}"#, key: "]", [.command, .shift]))
        windowMenu.addItem(switchItem(L.previousTab, #"{"type":"tabStep","delta":-1}"#, key: "[", [.command, .shift]))
        windowMenu.addItem(switchItem(L.nextPane, #"{"type":"paneStep","delta":1}"#, key: "]", .command))
        windowMenu.addItem(switchItem(L.previousPane, #"{"type":"paneStep","delta":-1}"#, key: "[", .command))
        let tabs = NSMenu(title: L.goToTab)
        for number in 1...9 {
            let title = number == 9 ? L.lastTab : L.tabNumber(number)
            tabs.addItem(switchItem(title, #"{"type":"tab","number":\#(number)}"#, key: String(number), .command))
        }
        windowMenu.addItem(submenu(tabs, title: L.goToTab))
        main.addItem(submenu(windowMenu, title: L.window))
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
