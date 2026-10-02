import Combine
import Foundation

/// Where the viewer got to in one file.
struct WatchRecord: Codable, Hashable {
    var position: Double = 0        // seconds
    var duration: Double = 0        // seconds
    var watched: Bool = false
    var lastPlayed: Date = .now

    /// 0...1. Meaningless until we know the duration.
    var progress: Double {
        guard duration > 0 else { return 0 }
        return min(max(position / duration, 0), 1)
    }

    /// Past this point the episode counts as seen — the rest is credits.
    static let completionThreshold = 0.92

    /// Below this, "reprendre" would just mean "recommencer": the offer is
    /// noise and the seek is pointless. It used to be 30 seconds, which was
    /// long enough to lose a real viewing — an episode watched for a quarter
    /// of a minute appeared nowhere and restarted from zero.
    static let minimumResumeSeconds: Double = 5

    /// Don't offer to resume a few seconds in, or once effectively finished.
    var isResumable: Bool {
        !watched && position > Self.minimumResumeSeconds
            && progress < Self.completionThreshold
    }

    var formattedPosition: String {
        let total = Int(position)
        let h = total / 3600, m = (total % 3600) / 60, s = total % 60
        return h > 0 ? String(format: "%d:%02d:%02d", h, m, s)
                     : String(format: "%d:%02d", m, s)
    }
}

/// Watched state and resume positions, persisted across launches.
@MainActor
final class WatchStore: ObservableObject {
    static let shared = WatchStore()

    @Published private(set) var records: [String: WatchRecord] = [:]

    private let fileURL: URL
    private var saveTask: Task<Void, Never>?

    private init() {
        fileURL = AppSettings.supportDirectory.appendingPathComponent("watch-state.json")
        if let data = try? Data(contentsOf: fileURL),
           let decoded = try? JSONDecoder().decode([String: WatchRecord].self, from: data) {
            records = decoded
        }
    }

    func record(for path: String) -> WatchRecord? { records[path] }

    func progress(for item: MediaItem) -> Double { records[item.id]?.progress ?? 0 }
    func isWatched(_ item: MediaItem) -> Bool { records[item.id]?.watched ?? false }
    func resumePosition(for item: MediaItem) -> Double? {
        guard let r = records[item.id], r.isResumable else { return nil }
        return r.position
    }

    /// Called repeatedly while VLC plays.
    func update(path: String, position: Double, duration: Double) {
        var r = records[path] ?? WatchRecord()
        r.position = position
        if duration > 0 { r.duration = duration }
        r.lastPlayed = .now
        if r.progress >= WatchRecord.completionThreshold { r.watched = true }
        records[path] = r
        scheduleSave()
    }

    func setWatched(_ watched: Bool, for item: MediaItem) {
        var r = records[item.id] ?? WatchRecord()
        r.watched = watched
        r.lastPlayed = .now
        // Marking unwatched should also clear the resume point.
        if !watched { r.position = 0 } else if r.duration > 0 { r.position = r.duration }
        records[item.id] = r
        scheduleSave()
    }

    func setWatched(_ watched: Bool, for items: [MediaItem]) {
        for item in items { setWatched(watched, for: item) }
    }

    func clearProgress(for item: MediaItem) {
        records.removeValue(forKey: item.id)
        scheduleSave()
    }

    // MARK: Season-level rollup

    func watchedCount(in group: SeasonGroup) -> Int {
        group.episodes.reduce(0) { $0 + (isWatched($1) ? 1 : 0) }
    }

    /// How far through the season, counting a part-watched episode as partial.
    func progress(in group: SeasonGroup) -> Double {
        guard !group.episodes.isEmpty else { return 0 }
        let total = group.episodes.reduce(0.0) { sum, ep in
            sum + (isWatched(ep) ? 1.0 : progress(for: ep))
        }
        return total / Double(group.episodes.count)
    }

    /// The episode to offer when resuming a season: the one in progress,
    /// else the first unwatched one. Nil once the season is finished.
    func nextEpisode(in group: SeasonGroup) -> MediaItem? {
        if let inProgress = group.episodes.first(where: { resumePosition(for: $0) != nil }) {
            return inProgress
        }
        return group.episodes.first { !isWatched($0) }
    }

    /// When the viewer last touched any episode of the season, nil if never.
    /// Doubles as "has this season been started?".
    func lastPlayed(in group: SeasonGroup) -> Date? {
        group.episodes.compactMap { records[$0.id]?.lastPlayed }.max()
    }

    /// A season begun but not finished. It belongs under "Reprendre" even when
    /// no single file is mid-play: finishing episode 3 of 26 leaves the series
    /// to continue, not a series to forget.
    func isSeasonInProgress(_ group: SeasonGroup) -> Bool {
        lastPlayed(in: group) != nil && nextEpisode(in: group) != nil
    }

    // MARK: Persistence

    private func scheduleSave() {
        saveTask?.cancel()
        saveTask = Task { [weak self] in
            try? await Task.sleep(nanoseconds: 1_500_000_000)
            guard !Task.isCancelled else { return }
            self?.saveNow()
        }
    }

    func saveNow() {
        guard let data = try? JSONEncoder().encode(records) else { return }

        // Keep the previous file as a backup before overwriting. Watch history
        // is hand-curated and irreplaceable; a rollback beats a rebuild.
        // Never let an empty state overwrite a populated backup.
        let backup = fileURL.deletingLastPathComponent()
            .appendingPathComponent("watch-state.backup.json")
        if !records.isEmpty, let previous = try? Data(contentsOf: fileURL), !previous.isEmpty {
            try? previous.write(to: backup, options: .atomic)
        }
        try? data.write(to: fileURL, options: .atomic)
    }

    /// Restore the previous save. Returns the number of entries recovered.
    @discardableResult
    func restoreFromBackup() -> Int {
        let backup = fileURL.deletingLastPathComponent()
            .appendingPathComponent("watch-state.backup.json")
        guard let data = try? Data(contentsOf: backup),
              let decoded = try? JSONDecoder().decode([String: WatchRecord].self, from: data)
        else { return 0 }
        records = decoded
        saveNow()
        return decoded.count
    }
}
