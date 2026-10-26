import Foundation
import Testing
@testable import Biblifilm

/// Runs the parser over a real folder and flags names that came out wrong.
/// Off unless asked for, since it depends on what is on this Mac:
///
///     BIBLIFILM_SWEEP_DIR=~/Downloads/Films Tests/run-unit.sh --filter Sweep
///
/// Read-only: the folder is listed, never touched.
@Suite("Balayage d'un vrai dossier",
       .enabled(if: ProcessInfo.processInfo.environment["BIBLIFILM_SWEEP_DIR"] != nil))
struct LibrarySweepTests {

    /// Release words that should never survive into a title.
    private static let leftovers: Set<String> = [
        "1080p", "720p", "2160p", "x264", "x265", "h264", "hevc", "bluray", "webrip",
        "web-dl", "multi", "vostfr", "vff", "truefrench", "ac3", "eac3", "dts", "aac",
        "10bit", "hdlight", "av1", "e-ac-3", "hdma", "mhd",
    ]

    @Test func everyFileGetsAPlausibleName() {
        let root = URL(fileURLWithPath: (ProcessInfo.processInfo
            .environment["BIBLIFILM_SWEEP_DIR"]! as NSString).expandingTildeInPath)
        let items = LibraryScanner.scan(root: root, options: .init(includeAudio: false,
                                                                   skipSmallFiles: false))
        #expect(!items.isEmpty, "nothing found under \(root.path)")

        for item in items {
            let p = item.parsed
            if ProcessInfo.processInfo.environment["BIBLIFILM_SWEEP_VERBOSE"] != nil {
                print("\(p.kind.rawValue)\t\(p.title)\t\(p.year.map(String.init) ?? "")\t"
                      + "S\(p.season ?? -1)E\(p.episode ?? p.absoluteEpisode ?? -1)\t\(item.filename)")
            }
            let words = p.title.lowercased().split(separator: " ").map(String.init)
            let junk = words.filter(Self.leftovers.contains)
            #expect(!p.title.isEmpty, "empty title: \(item.filename)")
            #expect(junk.isEmpty, "release junk \(junk) in “\(p.title)” ← \(item.filename)")
            let raw = item.url.deletingPathExtension().lastPathComponent
            #expect(p.title != raw || !raw.contains(where: { $0 == "." || $0 == "_" }),
                    "title is the raw filename: \(item.filename)")
            if p.kind == .episode, p.special == nil {
                #expect(p.episode != nil || p.absoluteEpisode != nil,
                        "episode without a number: \(item.filename)")
            }
            if let year = p.year {
                #expect((1900...2100).contains(year), "odd year \(year): \(item.filename)")
            }
        }

        // Every episode in a show's own folder should land in one series, not
        // several. The root is left out: loose files there are anything.
        let shows = Dictionary(grouping: items.filter { $0.kind == .episode },
                               by: { $0.url.deletingLastPathComponent().path })
        for (folder, episodes) in shows where folder != root.path {
            let keys = Set(episodes.filter { $0.parsed.special == nil }.map(\.parsed.showKey))
            #expect(keys.count <= 1, "one folder, several shows \(keys): \(folder)")
        }
    }
}
