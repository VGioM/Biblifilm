import Foundation

/// Turns a messy release filename into a clean title + season/episode guess.
///
/// Handles the common shapes:
///   `[DragonMax] Nicky Larson - 001.mkv`        -> episode, absolute #1
///   `Breaking.Bad.S01E02.1080p.WEB-DL.mkv`      -> episode S1E2
///   `The Wire 3x04 - Amsterdam.avi`             -> episode S3E4
///   `Inception.2010.1080p.BluRay.x264.mkv`      -> movie, 2010
///   `Le Fabuleux Destin d'Amélie Poulain (2001).mp4` -> movie, 2001
enum FilenameParser {

    /// Extensions VLC plays. Video first, then audio.
    static let videoExtensions: Set<String> = [
        "mkv", "mp4", "m4v", "avi", "mov", "wmv", "flv", "webm", "mpg", "mpeg",
        "mpe", "m2v", "m2ts", "mts", "ts", "vob", "ogv", "ogm", "3gp", "3g2",
        "divx", "xvid", "rmvb", "rm", "asf", "f4v", "mxf", "nsv", "iso", "dv",
        "amv", "mp2", "mpv", "qt", "yuv", "m4p"
    ]

    static let audioExtensions: Set<String> = [
        "mp3", "flac", "m4a", "aac", "wav", "aiff", "aif", "ogg", "oga", "opus",
        "wma", "alac", "ape", "wv", "mka", "dsf", "dff", "mid", "midi", "it",
        "mod", "s3m", "xm", "spx", "tta", "voc", "au"
    ]

    static var playableExtensions: Set<String> { videoExtensions.union(audioExtensions) }

    /// Tokens that describe the *release*, never the title. Stripped from the name.
    private static let junkTokens: Set<String> = [
        // resolution / source
        "480p", "576p", "720p", "1080p", "1080i", "2160p", "4k", "uhd", "hd", "sd",
        "bluray", "blu-ray", "brrip", "bdrip", "bdremux", "remux", "webrip", "web-dl",
        "webdl", "web", "hdtv", "pdtv", "dvdrip", "dvdscr", "dvd", "hdrip", "camrip",
        "cam", "ts", "tc", "r5", "vhsrip", "satrip", "dsr", "hdcam",
        // codecs
        "x264", "x265", "h264", "h265", "h", "avc", "hevc", "xvid", "divx", "10bit",
        "8bit", "10bits", "hi10p", "hdr", "hdr10", "dovi", "dv", "sdr",
        // audio
        "aac", "aac2", "ac3", "eac3", "dts", "dtshd", "truehd", "atmos", "flac",
        "mp3", "opus", "ddp", "ddp5", "dd5", "dd", "2ch", "6ch",
        // language / tagging
        "multi", "multi3", "vostfr", "vost", "vf", "vff", "vfq", "vfi", "vo",
        "french", "truefrench", "english", "eng", "fre", "fr", "jap", "jpn",
        "subfrench", "subbed", "dubbed", "dual", "audio",
        // misc release noise
        "repack", "proper", "rerip", "internal", "limited", "unrated", "extended",
        "remastered", "directors", "cut", "theatrical", "complete", "integrale",
        "final", "uncensored", "bd", "raw", "batch", "nf", "amzn", "dsnp", "atvp",
        "hmax", "hulu", "itunes", "criterion", "version", "longue", "vlong",
        "subforced", "subs", "sub", "hdlight", "light", "uncut", "3d", "sbs",
        "readnfo", "hardcoded", "dubbed"
    ]

    private static let episodePatterns: [String] = [
        #"[sS](\d{1,2})[\s._-]*[eE](\d{1,3})"#,           // S01E02, s1 e2
        #"(?<![0-9a-zA-Z])(\d{1,2})[xX](\d{1,3})(?![0-9])"#, // 3x04
        #"[sS]aison[\s._-]*(\d{1,2})[\s._-]*[eEéÉ](?:p(?:isode)?)?[\s._-]*(\d{1,3})"#, // Saison 2 Episode 5
        #"[sS]eason[\s._-]*(\d{1,2})[\s._-]*[eE](?:p(?:isode)?)?[\s._-]*(\d{1,3})"#
    ]

    /// Markers for material attached to a series but not part of its episode
    /// numbering. Detected before episode parsing, because "OAV - 01" otherwise
    /// looks exactly like absolute episode 1 and collides with the real one.
    /// A special is only recognised after a dash separator, the shape release
    /// groups actually use ("Nicky Larson - OAV - 01 - Bay City Wars").
    /// Without that, ordinary titles like "The Movie Critic" or
    /// "Film Noir Collection" would be mistaken for specials.
    private static let separator = #"[\s._]+[-–][\s._]*"#

    /// Unambiguous words: these never appear inside a normal title.
    private static let strongMarkers: [(word: String, kind: SpecialKind)] = [
        (#"t[ée]l[ée]films?"#, .film),
        (#"oavs?|ovas?"#, .oav),
        (#"ncop|nced|openings?|endings?|g[ée]n[ée]riques?"#, .extra),
        (#"bonus|making[\s._-]?of"#, .extra)
    ]

    /// Vague words: only a special when a number follows ("Film 01").
    private static let numberedMarkers: [(word: String, kind: SpecialKind)] = [
        (#"films?"#, .film),
        (#"movies?"#, .film),
        (#"sp[ée]ciaux|sp[ée]ciales?|specials?"#, .special)
    ]

    private static var specialMarkers: [(pattern: String, kind: SpecialKind)] {
        strongMarkers.map { ("(?i)" + separator + "(?:\($0.word))(?![a-z])", $0.kind) }
        + numberedMarkers.map {
            ("(?i)" + separator + "(?:\($0.word))(?![a-z])(?=[\\s._-]*\\d)", $0.kind)
        }
    }

    /// Bare episode number with no season: "- 001", "Ep 12", "E12", "#5".
    private static let absolutePatterns: [String] = [
        #"[\s._-]-[\s._]*(\d{1,4})(?:v\d)?(?=[\s._\[(]|$)"#,   // " - 001"
        #"(?<![a-zA-Z0-9])[eEéÉ](?:[pP](?:isode|is)?)?[\s._-]*(\d{1,4})(?![0-9])"#, // "Episode 12" / "Ep12" / "E12"
        #"(?<![a-zA-Z0-9])#[\s._]*(\d{1,4})(?![0-9])"#           // "#12"
    ]

    /// - Parameter folderHint: parent directory name, used as the title when the
    ///   filename alone carries none (e.g. `Kaamelott/Saison 2 Episode 5.mkv`).
    static func parse(url: URL, folderHint: String? = nil) -> ParsedName {
        let ext = url.pathExtension.lowercased()
        // macOS stores filenames decomposed (NFD): "Téléfilm" arrives as
        // "e" + combining acute, which no [ée] class can match. Compose first.
        let raw = url.deletingPathExtension().lastPathComponent
            .precomposedStringWithCanonicalMapping

        var work = raw
        var releaseGroup: String?

        // 1. Pull out and remember bracketed release groups: [DragonMax], (Group)
        if let g = firstMatch(in: work, pattern: #"^\[([^\]]{1,30})\]"#, group: 1) {
            releaseGroup = g
        }
        work = replaceAll(work, pattern: #"\[[^\]]*\]"#, with: " ")
        work = replaceAll(work, pattern: #"\{[^\}]*\}"#, with: " ")

        // 2. Year. A parenthesised year is authoritative -- it distinguishes the
        //    release year from a number inside the title ("Blade Runner 2049 (2017)").
        var year: Int?
        var yearRange: Range<String.Index>?
        if let m = firstMatchRanges(in: work, pattern: #"\((19\d{2}|20\d{2})\)"#, groups: [1]) {
            year = Int(m.values[0])
            yearRange = m.full
        } else if let m = firstMatchRanges(in: work,
                                           pattern: #"(?<![0-9])(19\d{2}|20\d{2})(?![0-9])"#,
                                           groups: [1]) {
            year = Int(m.values[0])
            yearRange = m.full
        }

        // 3. Films, OAVs and bonus material: split the series name from the
        //    special's own title, and keep them out of episode numbering.
        var special: SpecialKind?
        var specialTitle: String?
        var specialNumber: Int?
        var specialRange: Range<String.Index>?
        for (pattern, kind) in specialMarkers {
            guard let m = firstMatchRanges(in: work, pattern: pattern, groups: []) else { continue }
            special = kind
            specialRange = m.full
            // Everything after the marker: an optional index, then a title.
            let tail = String(work[m.full.upperBound...])
            if let num = firstMatchRanges(in: tail, pattern: #"^[\s._\-]*(\d{1,3})(?![0-9])"#,
                                          groups: [1]) {
                specialNumber = Int(num.values[0])
                specialTitle = cleanTitle(String(tail[num.full.upperBound...]))
            } else if kind == .extra {
                // "Opening Francais" is a better row label than just "Francais".
                specialTitle = cleanTitle(String(work[m.full.lowerBound...]))
            } else {
                specialTitle = cleanTitle(tail)
            }
            if specialTitle?.isEmpty == true { specialTitle = nil }
            break
        }

        // 4. Season/episode.
        var season: Int?
        var episode: Int?
        var seRange: Range<String.Index>?
        for pattern in episodePatterns where special == nil {
            if let m = firstMatchRanges(in: work, pattern: pattern, groups: [1, 2]) {
                season = Int(m.values[0])
                episode = Int(m.values[1])
                seRange = m.full
                break
            }
        }

        // 4. Absolute numbering, only when no S/E was found.
        var absolute: Int?
        var absRange: Range<String.Index>?
        if season == nil, special == nil {
            for pattern in absolutePatterns {
                if let m = firstMatchRanges(in: work, pattern: pattern, groups: [1]) {
                    let n = Int(m.values[0])
                    // A 4-digit number that looks like a year is not an episode number.
                    if let n, !(n >= 1900 && n <= 2099) {
                        absolute = n
                        absRange = m.full
                        break
                    }
                }
            }
        }

        // 5. The title is whatever comes *before* the episode/year marker.
        var titlePart = work
        if let specialRange {
            titlePart = String(work[work.startIndex..<specialRange.lowerBound])
        } else if let r = seRange ?? absRange {
            titlePart = String(work[work.startIndex..<r.lowerBound])
        } else if let yearRange {
            titlePart = String(work[work.startIndex..<yearRange.lowerBound])
        }

        // Release names put the year before the episode tag ("Monster.2004.S01E61"),
        // so a trailing year belongs to the series, not to its title.
        if season != nil || absolute != nil {
            titlePart = replaceAll(titlePart,
                                   pattern: #"[\s._-]*(?<![0-9])(19\d{2}|20\d{2})(?![0-9])[\s._-]*$"#,
                                   with: " ")
        }

        var title = cleanTitle(titlePart)
        // A title that is empty, or that is only leftover episode wording, means the
        // filename carried no show name -- the containing folder usually does.
        if title.isEmpty || isNoiseOnly(title), let hint = folderHint {
            let hinted = cleanTitle(hint)
            if !hinted.isEmpty && !isNoiseOnly(hinted) { title = hinted }
        }
        if title.isEmpty { title = cleanTitle(work) }
        if title.isEmpty { title = raw }

        // 6. Decide the kind.
        let kind: MediaKind
        if audioExtensions.contains(ext) {
            kind = .audio
        } else if special != nil || season != nil || absolute != nil {
            kind = .episode
        } else if videoExtensions.contains(ext) {
            kind = .movie
        } else {
            kind = .other
        }

        return ParsedName(
            title: title,
            kind: kind,
            year: year,
            season: season,
            episode: episode,
            absoluteEpisode: absolute,
            releaseGroup: releaseGroup,
            special: special,
            specialTitle: specialTitle,
            specialNumber: specialNumber
        )
    }

    /// Cleans a title the user typed by hand in the correction sheet.
    ///
    /// Only a trailing year is removed, and returned separately so it can still
    /// narrow the search: "Pusher II (2004)" reaches TMDB as "Pusher II", which
    /// matches, where the parenthesised form matches nothing at all. Release
    /// junk is deliberately left alone here -- `cleanTitle` would turn a typed
    /// "The Cut" into "The".
    static func normalizeUserTitle(_ input: String) -> (title: String, year: Int?) {
        let composed = input.precomposedStringWithCanonicalMapping
            .trimmingCharacters(in: .whitespacesAndNewlines)
        let trailingYear = #"[\s(\[]*(?<![0-9])(19\d{2}|20\d{2})(?![0-9])[\s)\]]*$"#

        let year = firstMatch(in: composed, pattern: trailingYear, group: 1).flatMap(Int.init)

        var s = replaceAll(composed, pattern: trailingYear, with: " ")
        s = replaceAll(s, pattern: #"\s{2,}"#, with: " ")
        s = s.trimmingCharacters(in: CharacterSet(charactersIn: " -–—_.,:;()[]"))

        return (s.isEmpty ? composed : s, year)
    }

    /// True when a candidate title is nothing but season/episode wording.
    private static func isNoiseOnly(_ title: String) -> Bool {
        let stripped = replaceAll(
            title.lowercased(),
            pattern: #"\b(saison|season|episode|episodes|ep|epis|serie|series|s|e|vol|volume|part|partie|disc|disque|livre|book|cd|dvd)\b|\d+"#,
            with: " ")
        return stripped.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    /// Normalise separators, drop release junk, restore capitalisation.
    private static func cleanTitle(_ input: String) -> String {
        var s = input

        // Channel layouts (5.1, 7.1, 2.0) are noise, but bare digits are not:
        // strip them before dots become spaces.
        s = replaceAll(s, pattern: #"(?<![0-9])[0-9]\.[0-9](?![0-9])"#, with: " ")

        // A parenthesised aside is an alternate/translated title, not part of
        // the name we search for. (Years were already extracted by now.)
        s = replaceAll(s, pattern: #"\([^)]*\)"#, with: " ")

        // dots/underscores act as spaces in release names, but keep "S.W.A.T."-ish runs sane
        s = s.replacingOccurrences(of: "_", with: " ")
        s = replaceAll(s, pattern: #"(?<=[a-zA-Z0-9])\.(?=[a-zA-Z0-9])"#, with: " ")
        s = s.replacingOccurrences(of: ".", with: " ")
        s = replaceAll(s, pattern: #"[\(\)\[\]\{\}]"#, with: " ")

        // Cut at a trailing "-GROUP" tag only if it is the last token.
        s = replaceAll(s, pattern: #"[\s]-[\s]*[A-Za-z0-9]{2,15}[\s]*$"#, with: " ")

        // Drop junk tokens.
        let tokens = s.split(whereSeparator: { $0 == " " || $0 == "-" })
        var kept: [String] = []
        for t in tokens {
            let low = t.lowercased().trimmingCharacters(in: CharacterSet.punctuationCharacters)
            if low.isEmpty { continue }
            if junkTokens.contains(low) { continue }
            kept.append(String(t))
        }

        var out = kept.joined(separator: " ")
        out = replaceAll(out, pattern: #"\s{2,}"#, with: " ")
        out = out.trimmingCharacters(in: CharacterSet(charactersIn: " -–—_.,:;"))
        return out
    }

    // MARK: - tiny regex helpers

    private struct MatchResult {
        var full: Range<String.Index>
        var values: [String]
    }

    private static func firstMatchRanges(in s: String, pattern: String, groups: [Int]) -> MatchResult? {
        guard let re = try? NSRegularExpression(pattern: pattern) else { return nil }
        let ns = s as NSString
        guard let m = re.firstMatch(in: s, range: NSRange(location: 0, length: ns.length)) else { return nil }
        guard let full = Range(m.range, in: s) else { return nil }
        var values: [String] = []
        for g in groups {
            guard g < m.numberOfRanges, m.range(at: g).location != NSNotFound else { return nil }
            values.append(ns.substring(with: m.range(at: g)))
        }
        return MatchResult(full: full, values: values)
    }

    private static func firstMatch(in s: String, pattern: String, group: Int) -> String? {
        firstMatchRanges(in: s, pattern: pattern, groups: [group])?.values.first
    }

    private static func replaceAll(_ s: String, pattern: String, with repl: String) -> String {
        guard let re = try? NSRegularExpression(pattern: pattern) else { return s }
        let ns = s as NSString
        return re.stringByReplacingMatches(
            in: s, range: NSRange(location: 0, length: ns.length), withTemplate: repl)
    }
}
