import AppKit
import Foundation

/// Opens media in VLC, falling back to the system default player.
enum VLCLauncher {

    static let vlcBundleID = "org.videolan.vlc"

    /// Located VLC, if installed.
    static var vlcURL: URL? {
        if let u = NSWorkspace.shared.urlForApplication(withBundleIdentifier: vlcBundleID) {
            return u
        }
        let fallback = URL(fileURLWithPath: "/Applications/VLC.app")
        return FileManager.default.fileExists(atPath: fallback.path) ? fallback : nil
    }

    static var isVLCInstalled: Bool { vlcURL != nil }

    enum LaunchError: LocalizedError {
        case fileMissing(URL)
        case launchFailed(String)

        var errorDescription: String? {
            switch self {
            case .fileMissing(let u):
                return "Le fichier est introuvable :\n\(u.path)"
            case .launchFailed(let m):
                return "Impossible d'ouvrir le fichier : \(m)"
            }
        }
    }

    /// Play one file. Uses VLC when present, otherwise the default handler.
    @MainActor
    static func play(_ url: URL) async throws {
        guard FileManager.default.fileExists(atPath: url.path) else {
            throw LaunchError.fileMissing(url)
        }

        let config = NSWorkspace.OpenConfiguration()
        config.activates = true

        do {
            if let vlc = vlcURL {
                try await NSWorkspace.shared.open([url], withApplicationAt: vlc, configuration: config)
            } else {
                guard NSWorkspace.shared.open(url) else {
                    throw LaunchError.launchFailed("aucune application associée")
                }
            }
        } catch let e as LaunchError {
            throw e
        } catch {
            throw LaunchError.launchFailed(error.localizedDescription)
        }
    }

    /// Queue several files into one VLC playlist (used by "Play all").
    @MainActor
    static func play(_ urls: [URL]) async throws {
        let existing = urls.filter { FileManager.default.fileExists(atPath: $0.path) }
        guard let first = existing.first else {
            throw LaunchError.fileMissing(urls.first ?? URL(fileURLWithPath: "/"))
        }
        guard let vlc = vlcURL else { return try await play(first) }

        let config = NSWorkspace.OpenConfiguration()
        config.activates = true
        try await NSWorkspace.shared.open(existing, withApplicationAt: vlc, configuration: config)
    }

    static func revealInFinder(_ url: URL) {
        NSWorkspace.shared.activateFileViewerSelecting([url])
    }
}
