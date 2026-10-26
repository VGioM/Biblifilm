import Foundation
import Testing
@testable import Biblifilm

func makeItem(_ name: String, folder: String = "/media") -> MediaItem {
    let url = URL(fileURLWithPath: folder).appendingPathComponent(name)
    return MediaItem(url: url, parsed: FilenameParser.parse(url: url),
                     fileSize: 1, modified: .distantPast)
}

/// A store writing to its own temporary file -- never the real history.
@MainActor
private func makeStore() -> (WatchStore, URL) {
    let dir = FileManager.default.temporaryDirectory
        .appendingPathComponent("biblifilm-tests-\(UUID().uuidString)", isDirectory: true)
    try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    let url = dir.appendingPathComponent("watch-state.json")
    return (WatchStore(fileURL: url), url)
}

private func season(_ count: Int) -> SeasonGroup {
    let episodes = (1...count).map { makeItem(String(format: "Show.S01E%02d.mkv", $0)) }
    guard case .season(let group) = LibraryEntry.build(from: episodes).first else {
        fatalError("episodes did not group into a season")
    }
    return group
}

@Suite("Historique de visionnage")
@MainActor
struct WatchStoreTests {

    @Test func positionIsRecordedAndResumable() {
        let (store, _) = makeStore()
        let film = makeItem("Film.2001.mkv")
        store.update(path: film.id, position: 600, duration: 6000)
        #expect(store.resumePosition(for: film) == 600)
        #expect(!store.isWatched(film))
    }

    @Test func tooEarlyOrTooLateIsNotResumable() {
        let (store, _) = makeStore()
        let film = makeItem("Film.2001.mkv")
        store.update(path: film.id, position: 3, duration: 6000)
        #expect(store.resumePosition(for: film) == nil)
        store.update(path: film.id, position: 5900, duration: 6000)
        #expect(store.resumePosition(for: film) == nil)
        #expect(store.isWatched(film), "past 92 % counts as seen")
    }

    @Test func markingSeenClearsTheResumePoint() {
        let (store, _) = makeStore()
        let ep = makeItem("Show.S01E16.mkv")
        // Left at the credits, 89 % in -- then skipped to the next episode.
        store.update(path: ep.id, position: 1335, duration: 1508)
        store.setWatched(true, for: ep)
        #expect(store.isWatched(ep))
        #expect(store.resumePosition(for: ep) == nil)
        #expect(store.progress(for: ep) == 1)
    }

    @Test func markingSeenWithoutADurationStillClearsIt() {
        let (store, _) = makeStore()
        let ep = makeItem("Show.S01E01.mkv")
        store.update(path: ep.id, position: 100, duration: 0)
        store.setWatched(true, for: ep)
        #expect(store.resumePosition(for: ep) == nil)
    }

    @Test func markingUnseenRestartsFromTheTop() {
        let (store, _) = makeStore()
        let film = makeItem("Film.2001.mkv")
        store.update(path: film.id, position: 600, duration: 6000)
        store.setWatched(false, for: film)
        #expect(store.resumePosition(for: film) == nil)
        #expect(!store.isWatched(film))
    }

    @Test func aRewatchResumesWhereItWasLeft() {
        let (store, _) = makeStore()
        let film = makeItem("Film.2001.mkv")
        store.setWatched(true, for: film)
        // Started over and left half way.
        store.update(path: film.id, position: 2400, duration: 6000)
        #expect(store.isWatched(film), "still seen")
        #expect(store.resumePosition(for: film) == 2400)
    }

    // MARK: Season

    @Test func seasonStartsAtTheFirstEpisode() {
        let (store, _) = makeStore()
        let group = season(4)
        #expect(store.nextEpisode(in: group)?.parsed.episode == 1)
        #expect(!store.isSeasonInProgress(group))
    }

    @Test func seasonResumesTheEpisodeLeftLast() async throws {
        let (store, _) = makeStore()
        let group = season(20)
        let eps = group.episodes
        for ep in eps[0..<15] { store.setWatched(true, for: ep) }
        // The case from the real history: 16 and 17 left near the end
        // (before the skip button marked them), 18 and 19 seen, 20 left at 4 min.
        store.update(path: eps[15].id, position: 1335, duration: 1508)
        store.update(path: eps[16].id, position: 1252, duration: 1518)
        store.setWatched(true, for: eps[17])
        store.setWatched(true, for: eps[18])
        try await Task.sleep(nanoseconds: 2_000_000)
        store.update(path: eps[19].id, position: 253, duration: 1518)

        let next = try #require(store.nextEpisode(in: group))
        #expect(next.parsed.episode == 20)
        #expect(store.resumePosition(for: next) == 253)
    }

    @Test func afterAFinishedEpisodeComesTheNextUnseenOne() async throws {
        let (store, _) = makeStore()
        let group = season(5)
        let eps = group.episodes
        store.setWatched(true, for: eps[0])
        try await Task.sleep(nanoseconds: 2_000_000)
        store.setWatched(true, for: eps[1])
        #expect(store.nextEpisode(in: group)?.parsed.episode == 3)
        #expect(store.isSeasonInProgress(group))
    }

    @Test func unmarkingAnEpisodeMakesItTheNextOne() async throws {
        let (store, _) = makeStore()
        let group = season(5)
        store.setWatched(true, for: group.episodes)
        #expect(store.nextEpisode(in: group) == nil)
        try await Task.sleep(nanoseconds: 2_000_000)
        store.setWatched(false, for: group.episodes[2])
        #expect(store.nextEpisode(in: group)?.parsed.episode == 3)
    }

    @Test func finishedSeasonOffersNothing() {
        let (store, _) = makeStore()
        let group = season(3)
        store.setWatched(true, for: group.episodes)
        #expect(store.nextEpisode(in: group) == nil)
        #expect(!store.isSeasonInProgress(group))
        #expect(store.watchedCount(in: group) == 3)
        #expect(store.progress(in: group) == 1)
    }

    // MARK: Persistence

    @Test func survivesARelaunch() {
        let (store, url) = makeStore()
        let film = makeItem("Film.2001.mkv")
        store.update(path: film.id, position: 1234.5, duration: 6000)
        store.saveNow()

        let reloaded = WatchStore(fileURL: url)
        #expect(reloaded.resumePosition(for: film) == 1234.5)
    }

    @Test func previousSaveIsKeptAsABackup() {
        let (store, url) = makeStore()
        let a = makeItem("A.2001.mkv"), b = makeItem("B.2002.mkv")
        store.update(path: a.id, position: 100, duration: 1000)
        store.saveNow()
        store.update(path: b.id, position: 200, duration: 1000)
        store.saveNow()

        let fresh = WatchStore(fileURL: url)
        fresh.clearProgress(for: a)
        fresh.clearProgress(for: b)
        // Restoring brings back the save before last: A only.
        #expect(fresh.restoreFromBackup() == 1)
        #expect(fresh.resumePosition(for: a) == 100)
    }

    @Test func emptyStateNeverOverwritesTheBackup() throws {
        let (store, url) = makeStore()
        let a = makeItem("A.2001.mkv")
        store.update(path: a.id, position: 100, duration: 1000)
        store.saveNow()
        store.saveNow()                 // backup now holds A
        store.clearProgress(for: a)
        store.saveNow()                 // empty main file; backup must survive
        let backup = url.deletingLastPathComponent()
            .appendingPathComponent("watch-state.backup.json")
        let saved = try JSONDecoder().decode([String: WatchRecord].self,
                                             from: Data(contentsOf: backup))
        #expect(saved[a.id] != nil)
    }

    @Test func corruptFileStartsEmptyInsteadOfCrashing() throws {
        let (_, url) = makeStore()
        try Data("{not json".utf8).write(to: url)
        let store = WatchStore(fileURL: url)
        #expect(store.records.isEmpty)
    }
}

@Suite("Fiche de visionnage")
struct WatchRecordTests {
    @Test func progressIsClamped() {
        #expect(WatchRecord(position: 50, duration: 0).progress == 0)
        #expect(WatchRecord(position: 2000, duration: 1000).progress == 1)
        #expect(WatchRecord(position: -5, duration: 1000).progress == 0)
    }

    @Test("Position lisible", arguments: [
        (0.0, "0:00"), (65.9, "1:05"), (3600, "1:00:00"), (7384, "2:03:04"),
    ])
    func formattedPosition(_ seconds: Double, _ text: String) {
        #expect(WatchRecord(position: seconds).formattedPosition == text)
    }
}
