import Foundation
import Combine

/// User preferences, persisted in UserDefaults.
@MainActor
final class AppSettings: ObservableObject {
    static let shared = AppSettings()

    @Published var tmdbAPIKey: String {
        didSet { defaults.set(tmdbAPIKey, forKey: Keys.tmdbKey) }
    }
    @Published var libraryPath: String? {
        didSet { defaults.set(libraryPath, forKey: Keys.libraryPath) }
    }
    @Published var preferredLanguage: String {
        didSet { defaults.set(preferredLanguage, forKey: Keys.language) }
    }
    @Published var includeAudio: Bool {
        didSet { defaults.set(includeAudio, forKey: Keys.includeAudio) }
    }
    @Published var skipSmallFiles: Bool {
        didSet { defaults.set(skipSmallFiles, forKey: Keys.skipSmall) }
    }
    @Published var autoFetchMetadata: Bool {
        didSet { defaults.set(autoFetchMetadata, forKey: Keys.autoFetch) }
    }
    @Published var watchFolder: Bool {
        didSet { defaults.set(watchFolder, forKey: Keys.watchFolder) }
    }

    private let defaults = UserDefaults.standard

    private enum Keys {
        static let tmdbKey = "tmdbAPIKey"
        static let libraryPath = "libraryPath"
        static let language = "preferredLanguage"
        static let includeAudio = "includeAudio"
        static let skipSmall = "skipSmallFiles"
        static let autoFetch = "autoFetchMetadata"
        static let watchFolder = "watchFolder"
    }

    private init() {
        defaults.register(defaults: [
            Keys.language: "fr-FR",
            Keys.includeAudio: true,
            Keys.skipSmall: true,
            Keys.autoFetch: true,
            Keys.watchFolder: true
        ])
        tmdbAPIKey = defaults.string(forKey: Keys.tmdbKey) ?? ""
        libraryPath = defaults.string(forKey: Keys.libraryPath)
        preferredLanguage = defaults.string(forKey: Keys.language) ?? "fr-FR"
        includeAudio = defaults.bool(forKey: Keys.includeAudio)
        skipSmallFiles = defaults.bool(forKey: Keys.skipSmall)
        autoFetchMetadata = defaults.bool(forKey: Keys.autoFetch)
        watchFolder = defaults.bool(forKey: Keys.watchFolder)
    }

    var hasTMDBKey: Bool { !tmdbAPIKey.trimmingCharacters(in: .whitespaces).isEmpty }

    static let availableLanguages: [(code: String, label: String)] = [
        ("fr-FR", "Français"),
        ("en-US", "English"),
        ("es-ES", "Español"),
        ("de-DE", "Deutsch"),
        ("it-IT", "Italiano"),
        ("pt-BR", "Português"),
        ("ja-JP", "日本語")
    ]

    /// Application-support directory, created on demand.
    nonisolated static var supportDirectory: URL {
        let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first!
            .appendingPathComponent("Biblifilm", isDirectory: true)
        try? FileManager.default.createDirectory(at: base, withIntermediateDirectories: true)
        return base
    }
}
