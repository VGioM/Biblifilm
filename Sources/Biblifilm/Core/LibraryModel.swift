import AppKit
import Combine
import Foundation
import SwiftUI

/// Which sidebar entry is selected.
enum LibrarySection: Hashable {
    case all
    case continueWatching
    case movies
    case shows
    case audio
    case recent
    case unmatched
    case series(String)

    var title: String {
        switch self {
        case .all:       return "Tout"
        case .continueWatching: return "Reprendre"
        case .movies:    return "Films"
        case .shows:     return "Séries"
        case .audio:     return "Audio"
        case .recent:    return "Ajouts récents"
        case .unmatched: return "Non identifiés"
        case .series(let s): return s
        }
    }

    var symbol: String {
        switch self {
        case .all:       return "square.grid.2x2"
        case .continueWatching: return "play.circle"
        case .movies:    return "film"
        case .shows:     return "tv"
        case .audio:     return "music.note"
        case .recent:    return "clock"
        case .unmatched: return "questionmark.circle"
        case .series:    return "rectangle.stack"
        }
    }

    /// A string form for UserDefaults, so the app reopens on the same tab.
    var storageKey: String {
        switch self {
        case .all:       return "all"
        case .continueWatching: return "continueWatching"
        case .movies:    return "movies"
        case .shows:     return "shows"
        case .audio:     return "audio"
        case .recent:    return "recent"
        case .unmatched: return "unmatched"
        case .series(let s): return "series:" + s
        }
    }

    init?(storageKey key: String) {
        switch key {
        case "all":       self = .all
        case "continueWatching": self = .continueWatching
        case "movies":    self = .movies
        case "shows":     self = .shows
        case "audio":     self = .audio
        case "recent":    self = .recent
        case "unmatched": self = .unmatched
        default:
            guard key.hasPrefix("series:") else { return nil }
            self = .series(String(key.dropFirst("series:".count)))
        }
    }
}

enum SortOrder: String, CaseIterable, Identifiable {
    case title = "Titre"
    case dateAdded = "Date d'ajout"
    case size = "Taille"
    case kind = "Type"
    var id: String { rawValue }
}

/// Owns the scanned library, drives metadata lookups, and exposes filtered views.
@MainActor
final class LibraryModel: ObservableObject {

    @Published private(set) var items: [MediaItem] = []
    @Published var selection: LibrarySection = LibraryModel.savedSelection {
        didSet {
            guard selection != oldValue else { return }
            openedSeasonID = nil
            UserDefaults.standard.set(selection.storageKey, forKey: Self.selectionKey)
        }
    }

    private static let selectionKey = "lastSelection"
    private static var savedSelection: LibrarySection {
        UserDefaults.standard.string(forKey: selectionKey)
            .flatMap(LibrarySection.init(storageKey:)) ?? .all
    }
    @Published var selectedItemID: String?
    /// Right-hand info panel. Closing it only hides it for the current
    /// selection — picking another poster brings it straight back.
    @Published var inspectorVisible = true
    /// Set while the user has drilled into one season's episode list.
    @Published var openedSeasonID: String?
    @Published var searchText: String = "" {
        didSet { if searchText != oldValue { openedSeasonID = nil } }
    }
    @Published var sortOrder: SortOrder = .title

    @Published private(set) var isScanning = false
    @Published private(set) var scanProgress: String = ""
    @Published private(set) var isFetchingMetadata = false
    @Published private(set) var metadataProgress: Double = 0
    @Published var errorMessage: String?

    private var scanTask: Task<Void, Never>?
    private var metadataTask: Task<Void, Never>?
    private let settings = AppSettings.shared

    private let watcher = FolderWatcher()
    private var pendingRefresh: Task<Void, Never>?
    private var cancellables = Set<AnyCancellable>()
    private var lastKnownKey: String

    init() {
        lastKnownKey = AppSettings.shared.tmdbAPIKey.trimmingCharacters(in: .whitespaces)

        // Pasting an API key should immediately retry everything that failed
        // without one -- otherwise the recorded misses would hide the fix.
        // Debounced so it fires once the key is fully typed, not per keystroke.
        AppSettings.shared.$tmdbAPIKey
            .debounce(for: .seconds(2), scheduler: RunLoop.main)
            .sink { [weak self] newKey in
                guard let self else { return }
                let key = newKey.trimmingCharacters(in: .whitespaces)
                guard key != self.lastKnownKey, key.count >= 20 else { return }
                self.lastKnownKey = key
                self.retryUnmatched()
            }
            .store(in: &cancellables)
    }

    var libraryPath: String? { settings.libraryPath }

    var libraryName: String {
        guard let p = settings.libraryPath else { return "Bibliothèque" }
        return URL(fileURLWithPath: p).lastPathComponent
    }

    // MARK: - Derived collections

    /// Distinct series names, for the sidebar.
    var seriesNames: [String] {
        let names = Set(items.filter { $0.kind == .episode }.map(\.groupingTitle))
        return names.sorted { $0.localizedStandardCompare($1) == .orderedAscending }
    }

    var counts: [LibrarySection: Int] {
        [
            .all: items.count,
            .movies: items.filter { $0.kind == .movie }.count,
            .shows: items.filter { $0.kind == .episode }.count,
            .audio: items.filter { $0.kind == .audio }.count,
            .unmatched: items.filter { $0.metadata?.matched != true }.count,
            .continueWatching: continueWatchingEntries.count
        ]
    }

    /// Items for the current section, search text and sort order.
    var visibleItems: [MediaItem] {
        var result: [MediaItem]
        switch selection {
        case .all:       result = items
        case .movies:    result = items.filter { $0.kind == .movie }
        case .shows:     result = items.filter { $0.kind == .episode }
        case .audio:     result = items.filter { $0.kind == .audio }
        case .unmatched: result = items.filter { $0.metadata?.matched != true }
        case .recent:
            result = items.sorted { $0.modified > $1.modified }
            return Array(result.prefix(60))
        case .continueWatching:
            return continueWatchingTargets
        case .series(let name):
            result = items.filter { $0.kind == .episode && $0.groupingTitle == name }
            return result.sorted { lhs, rhs in
                let l = (lhs.metadata?.season ?? lhs.parsed.season ?? 0,
                         lhs.metadata?.episode ?? lhs.parsed.episode ?? lhs.parsed.absoluteEpisode ?? 0)
                let r = (rhs.metadata?.season ?? rhs.parsed.season ?? 0,
                         rhs.metadata?.episode ?? rhs.parsed.episode ?? rhs.parsed.absoluteEpisode ?? 0)
                return l < r
            }
        }

        let query = searchText.trimmingCharacters(in: .whitespaces)
        if !query.isEmpty {
            result = result.filter { item in
                item.displayTitle.localizedCaseInsensitiveContains(query)
                || item.groupingTitle.localizedCaseInsensitiveContains(query)
                || item.filename.localizedCaseInsensitiveContains(query)
            }
        }

        switch sortOrder {
        case .title:
            result.sort { $0.displayTitle.localizedStandardCompare($1.displayTitle) == .orderedAscending }
        case .dateAdded:
            result.sort { $0.modified > $1.modified }
        case .size:
            result.sort { $0.fileSize > $1.fileSize }
        case .kind:
            result.sort {
                $0.kind.rawValue == $1.kind.rawValue
                    ? $0.displayTitle.localizedStandardCompare($1.displayTitle) == .orderedAscending
                    : $0.kind.rawValue < $1.kind.rawValue
            }
        }
        return result
    }

    /// The "Reprendre" tiles, most recently watched first.
    ///
    /// A film qualifies while it is mid-play. A season qualifies from the
    /// moment it is started until its last episode is watched -- so finishing
    /// an episode of a series advances the tile to the next one instead of
    /// clearing the series away. The tile carries the whole season, not just
    /// the episodes in progress, so its progress bar, "3/26 vus" and episode
    /// badge describe the season the viewer is actually working through.
    var continueWatchingEntries: [LibraryEntry] {
        let store = WatchStore.shared
        var dated: [(entry: LibraryEntry, lastPlayed: Date)] = []

        for entry in LibraryEntry.build(from: items) {
            switch entry {
            case .single(let item):
                guard store.resumePosition(for: item) != nil,
                      let record = store.record(for: item.id) else { continue }
                dated.append((entry, record.lastPlayed))
            case .season(let group):
                guard store.isSeasonInProgress(group),
                      let last = store.lastPlayed(in: group) else { continue }
                dated.append((entry, last))
            }
        }

        dated.sort { $0.lastPlayed > $1.lastPlayed }
        return dated.map(\.entry)
    }

    /// What each "Reprendre" tile would play next, in the same order.
    var continueWatchingTargets: [MediaItem] {
        continueWatchingEntries.compactMap { entry in
            switch entry {
            case .single(let item):  return item
            case .season(let group): return WatchStore.shared.nextEpisode(in: group)
            }
        }
    }

    /// The grid's tiles: one per film, one per season.
    var visibleEntries: [LibraryEntry] {
        // "Reprendre" keeps whole seasons and its own recency order.
        if selection == .continueWatching {
            let query = searchText.trimmingCharacters(in: .whitespaces)
            guard !query.isEmpty else { return continueWatchingEntries }
            return continueWatchingEntries.filter {
                $0.displayTitle.localizedCaseInsensitiveContains(query)
            }
        }
        var entries = LibraryEntry.build(from: visibleItems)

        switch sortOrder {
        case .title:
            // Sort on the series/film title, then chronologically by season --
            // never on the individual episode title.
            entries.sort {
                let c = $0.sortTitle.localizedStandardCompare($1.sortTitle)
                return c == .orderedSame ? $0.seasonNumber < $1.seasonNumber : c == .orderedAscending
            }
        case .dateAdded:
            entries.sort { $0.latestModified > $1.latestModified }
        case .size:
            entries.sort { $0.totalSize > $1.totalSize }
        case .kind:
            entries.sort {
                if $0.kind.rawValue != $1.kind.rawValue { return $0.kind.rawValue < $1.kind.rawValue }
                let c = $0.sortTitle.localizedStandardCompare($1.sortTitle)
                return c == .orderedSame ? $0.seasonNumber < $1.seasonNumber : c == .orderedAscending
            }
        }
        return entries
    }

    /// The season currently drilled into, if any.
    var openedSeason: SeasonGroup? {
        guard let id = openedSeasonID else { return nil }
        for case .season(let group) in LibraryEntry.build(from: items) where group.id == id {
            return group
        }
        return nil
    }

    func open(_ group: SeasonGroup) {
        openedSeasonID = group.id
        select(group.episodes.first?.id)
    }

    /// Selecting anything reopens the info panel; deselecting closes it.
    func select(_ id: String?) {
        selectedItemID = id
        if id != nil { inspectorVisible = true }
    }

    func clearSelection() {
        selectedItemID = nil
    }

    func closeSeason() {
        openedSeasonID = nil
        selectedItemID = nil
    }

    var selectedItem: MediaItem? {
        guard let id = selectedItemID else { return nil }
        return items.first { $0.id == id }
    }

    // MARK: - Folder choice

    func chooseFolder() {
        let panel = NSOpenPanel()
        panel.canChooseFiles = false
        panel.canChooseDirectories = true
        panel.allowsMultipleSelection = false
        panel.prompt = "Choisir"
        panel.message = "Choisissez le dossier contenant vos films et séries"
        if let current = settings.libraryPath {
            panel.directoryURL = URL(fileURLWithPath: current)
        }
        guard panel.runModal() == .OK, let url = panel.url else { return }
        settings.libraryPath = url.path
        rescan()
    }

    // MARK: - Scanning

    /// The tab remembered from last time may have no row any more: a series
    /// that left the folder, or a "Reprendre" with nothing left to resume.
    private func dropStaleSelection() {
        switch selection {
        case .series(let name) where !seriesNames.contains(name):
            selection = .all
        case .continueWatching where continueWatchingEntries.isEmpty,
             .unmatched where counts[.unmatched] == 0,
             .audio where !settings.includeAudio:
            selection = .all
        default:
            break
        }
    }

    func rescanIfNeeded() {
        if items.isEmpty, settings.libraryPath != nil { rescan() }
    }

    func rescan() {
        guard let path = settings.libraryPath else { return }
        scanTask?.cancel()
        metadataTask?.cancel()

        let root = URL(fileURLWithPath: path)
        let options = LibraryScanner.Options(
            includeAudio: settings.includeAudio,
            skipSmallFiles: settings.skipSmallFiles)

        isScanning = true
        scanProgress = "Analyse…"
        selectedItemID = nil

        scanTask = Task { [weak self] in
            guard let self else { return }

            // Built here so the detached scan captures only a Sendable closure,
            // never the model itself.
            let report: @Sendable (Int) -> Void = { [weak self] count in
                Task { @MainActor in
                    self?.scanProgress = "\(count) fichiers trouvés…"
                }
            }

            let found = await Task.detached(priority: .userInitiated) { () -> [MediaItem] in
                LibraryScanner.scan(root: root, options: options, progress: report)
            }.value

            guard !Task.isCancelled else { return }

            self.items = found
            self.isScanning = false
            self.scanProgress = ""
            self.dropStaleSelection()

            // Apply anything already cached, then fetch what's missing.
            await self.applyCachedMetadata()
            if self.settings.autoFetchMetadata { self.fetchMetadata() }
            self.startWatching()
        }
    }

    // MARK: - Watching for new files

    /// Watch the library folder so files added later appear on their own.
    private func startWatching() {
        watcher.stop()
        guard settings.watchFolder, let path = settings.libraryPath else { return }
        watcher.start(path: path) { [weak self] in
            self?.scheduleRefresh()
        }
    }

    func updateWatching() {
        if settings.watchFolder { startWatching() } else { watcher.stop() }
    }

    /// Coalesce a burst of filesystem events into one refresh.
    private func scheduleRefresh() {
        pendingRefresh?.cancel()
        pendingRefresh = Task { [weak self] in
            try? await Task.sleep(nanoseconds: 4_000_000_000)
            guard !Task.isCancelled else { return }
            self?.refreshIncrementally()
        }
    }

    /// Re-walk the folder and merge, keeping metadata already resolved so the
    /// view doesn't flicker and nothing is looked up twice.
    func refreshIncrementally() {
        guard let path = settings.libraryPath, !isScanning else { return }
        let root = URL(fileURLWithPath: path)
        let options = LibraryScanner.Options(
            includeAudio: settings.includeAudio,
            skipSmallFiles: settings.skipSmallFiles)

        Task { [weak self] in
            guard let self else { return }
            let found = await Task.detached(priority: .utility) { () -> [MediaItem] in
                LibraryScanner.scan(root: root, options: options)
            }.value
            guard !Task.isCancelled else { return }

            let previous = Dictionary(self.items.map { ($0.id, $0) }, uniquingKeysWith: { a, _ in a })
            var merged: [MediaItem] = []
            var added = 0
            for var item in found {
                if let old = previous[item.id] {
                    item.metadata = old.metadata
                    item.lookupFailed = old.lookupFailed
                } else {
                    added += 1
                }
                merged.append(item)
            }

            // Nothing actually changed: leave the view alone.
            guard added > 0 || merged.count != self.items.count else { return }

            self.items = merged
            await self.applyCachedMetadata()
            if self.settings.autoFetchMetadata { self.fetchMetadata() }
        }
    }

    /// Re-run only the lookups that previously found nothing.
    func retryUnmatched() {
        guard !items.isEmpty else { return }
        Task { [weak self] in
            guard let self else { return }
            await MetadataService.shared.forgetUnmatched()
            for index in self.items.indices where self.items[index].metadata?.matched != true {
                self.items[index].metadata = nil
                self.items[index].lookupFailed = false
            }
            self.fetchMetadata()
        }
    }

    private func applyCachedMetadata() async {
        let language = settings.preferredLanguage
        var updated = items
        for index in updated.indices {
            if let hit = await MetadataService.shared.cached(updated[index].parsed, language: language) {
                updated[index].metadata = hit
            }
        }
        items = updated
    }

    // MARK: - Metadata

    /// Fetch metadata for every item still missing it, a few at a time.
    func fetchMetadata(force: Bool = false) {
        metadataTask?.cancel()

        let language = settings.preferredLanguage
        let snapshot = MetadataSettings(
            tmdbKey: settings.hasTMDBKey ? settings.tmdbAPIKey : nil,
            language: language)

        let targets = items.enumerated().filter { _, item in
            force || item.metadata == nil
        }.map { ($0.offset, $0.element.parsed) }

        guard !targets.isEmpty else { return }

        isFetchingMetadata = true
        metadataProgress = 0

        metadataTask = Task { [weak self] in
            guard let self else { return }
            if force { await MetadataService.shared.clearCache() }

            let total = Double(targets.count)
            var done = 0.0

            // Small concurrency window: fast, but polite to the APIs.
            await withTaskGroup(of: (Int, MediaMetadata?).self) { group in
                var iterator = targets.makeIterator()
                let maxConcurrent = 4

                func addNext(_ group: inout TaskGroup<(Int, MediaMetadata?)>) {
                    guard let (index, parsed) = iterator.next() else { return }
                    group.addTask {
                        let meta = try? await MetadataService.shared.metadata(
                            for: parsed, settings: snapshot)
                        return (index, meta)
                    }
                }

                for _ in 0..<maxConcurrent { addNext(&group) }

                while let (index, meta) = await group.next() {
                    if Task.isCancelled { break }
                    if index < self.items.count {
                        if let meta {
                            self.items[index].metadata = meta
                        } else {
                            self.items[index].metadata = MediaMetadata(matched: false)
                            self.items[index].lookupFailed = true
                        }
                    }
                    done += 1
                    self.metadataProgress = done / total

                    // Save as we go: a large library takes minutes, and quitting
                    // midway should not throw away what was already resolved.
                    if Int(done) % 25 == 0 { await MetadataService.shared.flush() }

                    addNext(&group)
                }
            }

            await MetadataService.shared.flush()
            self.isFetchingMetadata = false
            self.metadataProgress = 0
        }
    }

    private func metadataSnapshot() -> MetadataSettings {
        MetadataSettings(
            tmdbKey: settings.hasTMDBKey ? settings.tmdbAPIKey : nil,
            language: settings.preferredLanguage)
    }

    /// What to search with: the file's own parsed name, unless the user typed a
    /// correction. A typed title goes through the same tidy-up a filename gets,
    /// or "Pusher II (2004)" reaches TMDB with its parentheses and matches
    /// nothing at all.
    private func searchName(for item: MediaItem, overrideTitle: String?) -> ParsedName {
        var parsed = item.parsed
        guard let overrideTitle,
              !overrideTitle.trimmingCharacters(in: .whitespaces).isEmpty else { return parsed }
        let cleaned = FilenameParser.normalizeUserTitle(overrideTitle)
        parsed.title = cleaned.title
        if let year = cleaned.year { parsed.year = year }
        return parsed
    }

    /// Re-run the lookup for one item, e.g. after the user corrects the title.
    func refetch(_ item: MediaItem, overrideTitle: String? = nil) {
        guard let index = items.firstIndex(where: { $0.id == item.id }) else { return }
        let query = searchName(for: item, overrideTitle: overrideTitle)
        let snapshot = metadataSnapshot()
        let original = item.parsed

        items[index].metadata = nil
        items[index].lookupFailed = false

        Task { [weak self] in
            // Start from a clean slate, otherwise the cached (possibly wrong)
            // answer comes straight back.
            await MetadataService.shared.forget(original, language: snapshot.language)
            await MetadataService.shared.forget(query, language: snapshot.language)
            let meta = try? await MetadataService.shared.metadata(for: query, settings: snapshot)
            // File the answer under the item's own key, not the typed title's,
            // so it is still there after a relaunch.
            await MetadataService.shared.store(meta ?? MediaMetadata(matched: false),
                                               for: original, language: snapshot.language)
            guard let self, index < self.items.count else { return }
            self.items[index].metadata = meta ?? MediaMetadata(matched: false)
            self.items[index].lookupFailed = (meta == nil)
            await MetadataService.shared.flush()
        }
    }

    /// The matches to offer in the correction sheet, best guess first.
    func candidates(for item: MediaItem, overrideTitle: String?) async -> [MatchCandidate] {
        let query = searchName(for: item, overrideTitle: overrideTitle)
        let snapshot = metadataSnapshot()
        return (try? await MetadataService.shared.candidates(for: query, settings: snapshot)) ?? []
    }

    /// Pin the match the user chose. Ranking is out of the picture from here:
    /// the choice is written straight into the cache under the item's own key.
    ///
    /// A chosen series carries over to every episode of the same show -- picking
    /// the right title once per file would be a chore on a 60-episode season.
    func apply(_ candidate: MatchCandidate, to item: MediaItem, overrideTitle: String? = nil) {
        let query = searchName(for: item, overrideTitle: overrideTitle)
        let snapshot = metadataSnapshot()
        let siblings = item.kind == .episode
            ? items.filter { $0.id != item.id && $0.parsed.showKey == item.parsed.showKey }
            : []

        Task { [weak self] in
            for target in [item] + siblings {
                if Task.isCancelled { return }
                // Each episode keeps its own numbering; only the title we search
                // under comes from the correction.
                var ask = target.parsed
                ask.title = query.title
                if let year = query.year { ask.year = year }

                guard let meta = try? await MetadataService.shared.metadata(
                        for: candidate, parsed: ask, settings: snapshot), meta.matched else { continue }
                await MetadataService.shared.store(meta, for: target.parsed,
                                                   language: snapshot.language)
                guard let self,
                      let index = self.items.firstIndex(where: { $0.id == target.id }) else { return }
                self.items[index].metadata = meta
                self.items[index].lookupFailed = false
            }
            await MetadataService.shared.flush()
        }
    }

    // MARK: - Playback

    /// Play in the app's own player. Episodes queue the rest of their season
    /// so playback rolls on without going back to the library.
    func play(_ item: MediaItem, fromStart: Bool = false) {
        PlayerModel.shared.open(item, following: followers(of: item), fromStart: fromStart)
    }

    /// Hand a file to VLC.app instead -- the escape hatch when the built-in
    /// player cannot handle something.
    func openExternally(_ item: MediaItem) {
        Task {
            do { try await VLCLauncher.play(item.url) }
            catch { errorMessage = error.localizedDescription }
        }
    }

    /// The episodes that come after `item` in its own season.
    private func followers(of item: MediaItem) -> [MediaItem] {
        guard item.kind == .episode else { return [] }
        for case .season(let group) in LibraryEntry.build(from: items)
        where group.episodes.contains(where: { $0.id == item.id }) {
            guard let index = group.episodes.firstIndex(where: { $0.id == item.id }) else { break }
            return Array(group.episodes[(index + 1)...])
        }
        return []
    }

    /// Play one tile. A season starts at the next unwatched episode.
    ///
    /// The queue comes from `play(_:)`, i.e. from the whole season, never from
    /// the tile: under "Reprendre" a tile holds only the episodes in progress,
    /// so queueing its contents meant an episode resumed there had nothing to
    /// roll on to.
    func play(_ entry: LibraryEntry) {
        switch entry {
        case .single(let item):
            play(item)
        case .season(let group):
            guard let start = WatchStore.shared.nextEpisode(in: group) ?? group.episodes.first
            else { return }
            play(start)
        }
    }

    func playAllVisible() {
        let items = visibleEntries.flatMap { entry -> [MediaItem] in
            switch entry {
            case .single(let item):  return [item]
            case .season(let group): return group.episodes
            }
        }
        guard let first = items.first else { return }
        PlayerModel.shared.open(first, following: Array(items.dropFirst()))
    }

    func reveal(_ item: MediaItem) { VLCLauncher.revealInFinder(item.url) }
}
