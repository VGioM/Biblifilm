import Foundation
import Testing
@testable import Biblifilm

private func parse(_ name: String, folder: String? = nil) -> ParsedName {
    FilenameParser.parse(url: URL(fileURLWithPath: "/media/" + name), folderHint: folder)
}

@Suite("Noms de fichiers")
struct FilenameParserTests {

    @Test("Épisodes SxxEyy, 3x04, Saison/Episode",
          arguments: [
            ("Breaking.Bad.S01E02.1080p.WEB-DL.mkv", "Breaking Bad", 1, 2),
            ("The Wire 3x04 - Amsterdam.avi", "The Wire", 3, 4),
            ("Sakamoto Days S01E20 MULTi 1080p WEB AV1 E-AC-3 -Tsundere-Raws.mkv", "Sakamoto Days", 1, 20),
            ("Kaamelott Saison 2 Episode 5.mkv", "Kaamelott", 2, 5),
            ("Show.Name.s02.e10.mkv", "Show Name", 2, 10),
          ])
    func seasonEpisode(_ file: String, _ title: String, _ season: Int, _ episode: Int) {
        let p = parse(file)
        #expect(p.kind == .episode)
        #expect(p.title == title)
        #expect(p.season == season)
        #expect(p.episode == episode)
    }

    @Test("Films avec année",
          arguments: [
            ("Inception.2010.1080p.BluRay.x264.mkv", "Inception", 2010),
            ("Le Fabuleux Destin d'Amélie Poulain (2001).mp4", "Le Fabuleux Destin d'Amélie Poulain", 2001),
            ("Blade Runner 2049 (2017).mkv", "Blade Runner 2049", 2017),
            ("Marionnette.2020.MULTI.1080p.WEB.H264-EXTREME.mkv", "Marionnette", 2020),
            ("Buddy.2026.VOSTFR.1080p.WEBRip.10.bits.EAC3.5.1.x265-GL0P.mkv", "Buddy", 2026),
            ("Il est de retour.2015.MULTi.1080p.BluRay.HEVC.DTS.HDMA.5.1-AKLHD.mkv", "Il est de retour", 2015),
          ])
    func movies(_ file: String, _ title: String, _ year: Int) {
        let p = parse(file)
        #expect(p.kind == .movie)
        #expect(p.title == title)
        #expect(p.year == year)
    }

    @Test func absoluteNumbering() {
        let p = parse("[DragonMax] Nicky Larson - 001.mkv")
        #expect(p.kind == .episode)
        #expect(p.title == "Nicky Larson")
        #expect(p.absoluteEpisode == 1)
        #expect(p.season == nil)
        #expect(p.releaseGroup == "DragonMax")
    }

    @Test func yearBeforeEpisodeBelongsToSeries() {
        let p = parse("Monster.2004.S01E61.mkv")
        #expect(p.title == "Monster")
        #expect(p.year == 2004)
        #expect(p.season == 1 && p.episode == 61)
    }

    @Test func aYearIsNeverAnAbsoluteEpisode() {
        let p = parse("Some Show - 2004.mkv")
        #expect(p.absoluteEpisode == nil)
    }

    @Test func specialsStayOutOfNumbering() {
        let oav = parse("Nicky Larson - OAV - 01 - Bay City Wars.mkv")
        #expect(oav.special == .oav)
        #expect(oav.specialNumber == 1)
        #expect(oav.specialTitle == "Bay City Wars")
        #expect(oav.absoluteEpisode == nil)
        #expect(oav.title == "Nicky Larson")

        // Ordinary titles that merely contain the words.
        #expect(parse("The Movie Critic.mkv").special == nil)
        #expect(parse("Film Noir Collection.mkv").special == nil)
    }

    @Test func decomposedAccentsStillMatch() {
        // macOS hands filenames over in NFD.
        let nfd = "Nicky Larson - Téléfilm - Goodbye.mkv".decomposedStringWithCanonicalMapping
        #expect(parse(nfd).special == .film)
    }

    @Test func folderGivesTheTitleWhenTheFileHasNone() {
        let p = parse("Saison 2 Episode 5.mkv", folder: "Kaamelott")
        #expect(p.title == "Kaamelott")
        #expect(p.season == 2 && p.episode == 5)
    }

    @Test func kinds() {
        #expect(parse("Song.flac").kind == .audio)
        #expect(parse("Notes.txt").kind == .other)
        #expect(parse("Some Film.mkv").kind == .movie)
    }

    @Test func showKeyIsSharedAcrossEpisodes() {
        #expect(parse("Breaking.Bad.S01E02.mkv").showKey == parse("Breaking Bad S03E10 720p.mkv").showKey)
    }

    @Test("Titre saisi à la main",
          arguments: [
            ("Pusher II (2004)", "Pusher II", 2004 as Int?),
            ("The Cut", "The Cut", nil),
            ("  Alien 1979 ", "Alien", 1979),
          ])
    func userTitle(_ input: String, _ title: String, _ year: Int?) {
        let n = FilenameParser.normalizeUserTitle(input)
        #expect(n.title == title)
        #expect(n.year == year)
    }
}
