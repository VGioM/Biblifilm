import Foundation

/// A source of online metadata.
protocol MetadataProvider: Sendable {
    var name: String { get }
    func lookup(_ parsed: ParsedName, language: String) async throws -> MediaMetadata?
}

enum MetadataError: LocalizedError {
    case badResponse(Int)
    case noKey

    var errorDescription: String? {
        switch self {
        case .badResponse(let c): return "Réponse du service : HTTP \(c)"
        case .noKey: return "Clé API TMDB manquante"
        }
    }
}

/// Shared JSON fetch used by every provider.
enum HTTP {
    static let session: URLSession = {
        let c = URLSessionConfiguration.default
        c.timeoutIntervalForRequest = 20
        c.waitsForConnectivity = false
        c.requestCachePolicy = .returnCacheDataElseLoad
        return URLSession(configuration: c)
    }()

    static func json<T: Decodable>(_ type: T.Type, from url: URL,
                                   headers: [String: String] = [:]) async throws -> T {
        var req = URLRequest(url: url)
        req.setValue("Biblifilm/1.0", forHTTPHeaderField: "User-Agent")
        for (k, v) in headers { req.setValue(v, forHTTPHeaderField: k) }

        let (data, response) = try await session.data(for: req)
        if let http = response as? HTTPURLResponse, !(200...299).contains(http.statusCode) {
            throw MetadataError.badResponse(http.statusCode)
        }
        return try JSONDecoder().decode(type, from: data)
    }

    static func encode(_ s: String) -> String {
        s.addingPercentEncoding(withAllowedCharacters: .urlQueryAllowed) ?? s
    }
}

/// Coordinates providers and caches results on disk so a rescan is instant.
actor MetadataService {
    static let shared = MetadataService()

    private var cache: [String: MediaMetadata] = [:]
    private var inFlight: [String: Task<MediaMetadata?, Error>] = [:]
    private let cacheURL: URL
    private var dirty = false

    init() {
        cacheURL = AppSettings.supportDirectory.appendingPathComponent("metadata-cache.json")
        if let data = try? Data(contentsOf: cacheURL),
           let decoded = try? JSONDecoder().decode([String: MediaMetadata].self, from: data) {
            cache = decoded
        }
    }

    /// Cache key includes the episode so each episode keeps its own title.
    private func key(_ p: ParsedName, language: String) -> String {
        var k = "\(p.showKey)|\(language)"
        if let s = p.season, let e = p.episode { k += "|s\(s)e\(e)" }
        else if let a = p.absoluteEpisode { k += "|a\(a)" }
        return k
    }

    func cached(_ p: ParsedName, language: String) -> MediaMetadata? {
        cache[key(p, language: language)]
    }

    /// Look up metadata, coalescing duplicate concurrent requests.
    func metadata(for parsed: ParsedName, settings: MetadataSettings) async throws -> MediaMetadata? {
        let k = key(parsed, language: settings.language)
        if let hit = cache[k] { return hit }
        if let running = inFlight[k] { return try await running.value }

        let task = Task<MediaMetadata?, Error> { [settings] in
            var providers: [MetadataProvider] = []
            if let key = settings.tmdbKey, !key.isEmpty {
                providers.append(TMDBProvider(apiKey: key))
            }
            providers.append(TVmazeProvider())

            for provider in providers {
                if Task.isCancelled { return nil }
                do {
                    if let found = try await provider.lookup(parsed, language: settings.language) {
                        return found
                    }
                } catch {
                    continue   // try the next provider
                }
            }
            return nil
        }
        inFlight[k] = task

        defer { inFlight[k] = nil }
        let result = try await task.value

        // Record misses too, so we don't hammer the network on every launch.
        let toStore = result ?? MediaMetadata(matched: false)
        cache[k] = toStore
        dirty = true
        return result
    }

    func flush() {
        guard dirty else { return }
        if let data = try? JSONEncoder().encode(cache) {
            try? data.write(to: cacheURL, options: .atomic)
            dirty = false
        }
    }

    /// Pin a result to an item's own key.
    ///
    /// A correction is filed under the file we corrected, never under the title
    /// the user typed: keying it by the typed title left the original key --
    /// and its wrong answer -- sitting in the cache, so the fix was forgotten
    /// on the next launch.
    func store(_ meta: MediaMetadata, for p: ParsedName, language: String) {
        cache[key(p, language: language)] = meta
        dirty = true
        flush()
    }

    /// The possible matches for the correction sheet. Only TMDB knows enough
    /// alternative titles to be worth offering a choice from, so an empty list
    /// here means "no key", not "nothing matches".
    func candidates(for parsed: ParsedName,
                    settings: MetadataSettings) async throws -> [MatchCandidate] {
        guard let key = settings.tmdbKey, !key.isEmpty else { return [] }
        return try await TMDBProvider(apiKey: key).candidates(parsed, language: settings.language)
    }

    /// Resolve one candidate the user picked. Deliberately skips the cache:
    /// the whole point is to overrule what is in it.
    func metadata(for candidate: MatchCandidate, parsed: ParsedName,
                  settings: MetadataSettings) async throws -> MediaMetadata? {
        guard let key = settings.tmdbKey, !key.isEmpty else { return nil }
        return try await TMDBProvider(apiKey: key)
            .metadata(for: candidate, parsed: parsed, language: settings.language)
    }

    /// Forget one entry so the next lookup goes back to the network. Without
    /// this, "rechercher les infos à nouveau" simply handed back the cached
    /// answer -- including a wrong match, which then stuck for good.
    func forget(_ p: ParsedName, language: String) {
        if cache.removeValue(forKey: key(p, language: language)) != nil {
            dirty = true
            flush()
        }
    }

    /// Drop only the recorded misses, so a newly-added API key can retry them
    /// without discarding everything already resolved.
    func forgetUnmatched() {
        let before = cache.count
        cache = cache.filter { $0.value.matched }
        if cache.count != before { dirty = true; flush() }
    }

    func clearCache() {
        cache.removeAll()
        dirty = true
        flush()
    }

    var cacheCount: Int { cache.count }
}

/// Immutable snapshot of settings, safe to pass into the actor.
struct MetadataSettings: Sendable {
    var tmdbKey: String?
    var language: String
}
