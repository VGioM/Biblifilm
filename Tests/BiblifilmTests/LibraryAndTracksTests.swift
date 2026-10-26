import Foundation
import Testing
@testable import Biblifilm

@Suite("Regroupement de la bibliothèque")
struct LibraryEntryTests {

    @Test func episodesGroupBySeasonAndFilmsStayAlone() {
        let items = [
            makeItem("Show.S01E02.mkv"), makeItem("Film.2001.mkv"),
            makeItem("Show.S01E01.mkv"), makeItem("Show.S02E01.mkv"),
        ]
        let entries = LibraryEntry.build(from: items)
        #expect(entries.count == 3)

        var seasons: [Int?: [Int?]] = [:]
        var singles = 0
        for entry in entries {
            switch entry {
            case .single: singles += 1
            case .season(let group): seasons[group.season] = group.episodes.map(\.parsed.episode)
            }
        }
        #expect(singles == 1)
        #expect(seasons[1] == [1, 2], "episodes come back in playback order")
        #expect(seasons[2] == [1])
    }

    @Test func specialsGetTheirOwnTile() {
        let items = [
            makeItem("Nicky Larson - 001.mkv"),
            makeItem("Nicky Larson - OAV - 01 - Bay City Wars.mkv"),
        ]
        let groups = LibraryEntry.build(from: items).compactMap { entry -> SeasonGroup? in
            if case .season(let g) = entry { return g } else { return nil }
        }
        #expect(groups.count == 2)
        #expect(groups.contains { $0.special == .oav })
    }

    @Test func episodeOrderUsesNumbersNotText() {
        let items = (1...12).reversed().map { makeItem("Show - \(String(format: "%03d", $0)).mkv") }
        guard case .season(let group) = LibraryEntry.build(from: items).first else {
            Issue.record("no season"); return
        }
        #expect(group.episodes.map(\.episodeNumber) == Array(1...12))
    }

    @Test func idsAreUniqueAcrossKinds() {
        let entries = LibraryEntry.build(from: [makeItem("A.S01E01.mkv"), makeItem("A.2001.mkv")])
        #expect(Set(entries.map(\.id)).count == entries.count)
    }
}

@Suite("Langue des pistes")
struct TrackLanguageTests {

    @Test("Langue reconnue", arguments: [
        ("Français VFF AC3 5.1 @ 448Kbps - [French]", "fr", "vff" as String?, false),
        ("Track 2 - [fre]", "fr", nil, false),
        ("VF - [FR]", "fr", nil, false),
        ("English - [English]", "en", nil, false),
        ("Français (forcés) - [French]", "fr", nil, true),
        ("Japanese", "ja", nil, false),
    ])
    func language(_ label: String, _ code: String, _ variant: String?, _ forced: Bool) throws {
        let lang = try #require(TrackLanguage(label: label))
        #expect(lang.code == code)
        #expect(lang.variant == variant)
        #expect(lang.forced == forced)
    }

    @Test func ambiguousWordsOutsideBracketsAreIgnored() {
        // "it" and "no" are English words before they are languages.
        #expect(TrackLanguage(label: "No commentary") == nil)
        #expect(TrackLanguage(label: "Director says it all") == nil)
    }

    @Test("Nom affiché", arguments: [
        ("Français VFi AC3 5.1 @ 448Kbps - [French]", "Français VFi AC3 5.1 @ 448Kbps"),
        ("Track 2 - [French]", "French"),
        ("Piste 1 - [Japanese]", "Japanese"),
        ("Commentaires", "Commentaires"),
    ])
    @MainActor
    func displayName(_ raw: String, _ shown: String) {
        #expect(PlayerModel.displayName(raw) == shown)
    }

    @Test("Timecode", arguments: [
        (0.0, "0:00"), (59.9, "0:59"), (61, "1:01"), (3661, "1:01:01"),
        (-1, "0:00"), (.nan, "0:00"), (.infinity, "0:00"),
    ])
    func timecode(_ seconds: Double, _ text: String) {
        #expect(formatTimecode(seconds) == text)
    }
}
