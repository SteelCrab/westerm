// Updates for westerm.app from GitHub Releases: find the newest release,
// download westerm-macos-app.zip, check it against its SHA-256 sum and
// version, and unpack it. The app delegate swaps the bundle and relaunches.
// WESTERM_RELEASES_API points at another copy of the release data (tests).

import CryptoKit
import Foundation

struct AppRelease {
    let version: String
    let page: URL?
    let archive: URL?
    let sums: URL?
}

enum UpdateError: LocalizedError {
    case noArchive, checksum, unpack, version

    var errorDescription: String? {
        switch self {
        case .noArchive: L.noAppDownload
        case .checksum: L.checksumMismatch
        case .unpack: L.cannotUnpack
        case .version: L.wrongVersion
        }
    }
}

enum Updater {
    static let archiveName = "westerm-macos-app.zip"

    static var api: URL {
        URL(string: ProcessInfo.processInfo.environment["WESTERM_RELEASES_API"]
            ?? "https://api.github.com/repos/SteelCrab/westerm/releases/latest")!
    }

    /// "1.2.3" or "v1.2.3-rc.1" as [1, 2, 3].
    static func numbers(_ text: String) -> [Int]? {
        let trimmed = text.hasPrefix("v") ? String(text.dropFirst()) : text
        let core = trimmed.split(whereSeparator: { $0 == "-" || $0 == "+" }).first.map(String.init) ?? ""
        let parts = core.split(separator: ".", omittingEmptySubsequences: false).map { Int($0) }
        guard parts.count == 3, parts.allSatisfy({ $0 != nil }) else { return nil }
        return parts.compactMap { $0 }
    }

    static func isNewer(_ candidate: String, than current: String) -> Bool {
        guard let candidate = numbers(candidate), let current = numbers(current) else { return false }
        for (new, old) in zip(candidate, current) where new != old {
            return new > old
        }
        return false
    }

    static func parse(_ data: Data) -> AppRelease? {
        guard let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let tag = json["tag_name"] as? String, numbers(tag) != nil else { return nil }
        let assets = json["assets"] as? [[String: Any]] ?? []
        func url(_ name: String) -> URL? {
            assets.first { $0["name"] as? String == name }
                .flatMap { $0["browser_download_url"] as? String }
                .flatMap(URL.init(string:))
        }
        return AppRelease(
            version: tag.hasPrefix("v") ? String(tag.dropFirst()) : tag,
            page: (json["html_url"] as? String).flatMap(URL.init(string:)),
            archive: url(archiveName),
            sums: url(archiveName + ".sha256")
        )
    }

    static func latest(done: @escaping (AppRelease?, Error?) -> Void) {
        var request = URLRequest(url: api)
        request.setValue("application/vnd.github+json", forHTTPHeaderField: "Accept")
        request.timeoutInterval = 20
        URLSession.shared.dataTask(with: request) { data, _, error in
            done(data.flatMap(parse), error)
        }.resume()
    }

    static func sha256(_ data: Data) -> String {
        SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
    }

    /// Downloads and checks the release; returns the unpacked westerm.app in a
    /// new temporary folder. Blocking: call it off the main thread.
    static func prepare(_ release: AppRelease) throws -> URL {
        guard let archiveURL = release.archive, let sumsURL = release.sums else { throw UpdateError.noArchive }
        let manager = FileManager.default
        let work = manager.temporaryDirectory.appendingPathComponent("westerm-update-\(UUID().uuidString)")
        try manager.createDirectory(at: work, withIntermediateDirectories: true)

        let archive = try Data(contentsOf: archiveURL)
        let sums = try String(contentsOf: sumsURL, encoding: .utf8)
        let expected = sums.split(whereSeparator: \.isWhitespace).first.map { $0.lowercased() } ?? ""
        guard !expected.isEmpty, expected == sha256(archive) else { throw UpdateError.checksum }
        let zip = work.appendingPathComponent(archiveName)
        try archive.write(to: zip)

        let ditto = Process()
        ditto.executableURL = URL(fileURLWithPath: "/usr/bin/ditto")
        ditto.arguments = ["-x", "-k", zip.path, work.path]
        try ditto.run()
        ditto.waitUntilExit()
        guard ditto.terminationStatus == 0 else { throw UpdateError.unpack }

        let app = work.appendingPathComponent("westerm.app")
        let version = Bundle(url: app)?.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String
        guard version == release.version else { throw UpdateError.version }
        return app
    }

    /// Puts `newApp` where `bundle` is. The old copy goes to a temporary
    /// folder (a running app can be moved), and comes back if the move fails.
    static func swap(_ newApp: URL, into bundle: URL) throws {
        let manager = FileManager.default
        let previous = manager.temporaryDirectory.appendingPathComponent("westerm-previous-\(UUID().uuidString).app")
        try manager.moveItem(at: bundle, to: previous)
        do {
            try manager.moveItem(at: newApp, to: bundle)
        } catch {
            try? manager.moveItem(at: previous, to: bundle)
            throw error
        }
    }
}
