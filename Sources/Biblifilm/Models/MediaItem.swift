import Foundation

/// What kind of thing we think a file is, based on its name.
enum MediaKind: String, Codable, Hashable {
    case movie
    case episode
    case audio
    case other

    var displayName: String {
        switch self {
        case .movie:   return "Film"
        case .episode: return "Série"
        case .audio:   return "Audio"
        case .other:   return "Autre"
        }
    }

    var symbol: String {
        switch self {
        case .movie:   return "film"
        case .episode: return "tv"
        case .audio:   return "music.note"
        case .other:   return "questionmark.square.dashed"
        }
    }
}

/// Material that belongs to a series without being a numbered episode:
/// films, OAVs, TV specials, opening/ending credits.
enum SpecialKind: String, Codable, Hashable {
    case film
    case oav
    case special
    case extra

    /// Specials of different kinds are collected into one tile per bucket.
    var bucketLabel: String {
        switch self {
        case .film, .oav, .special: return "Films et hors-séries"
        case .extra:                return "Bonus"
        }
    }

    var rowLabel: String {
        switch self {
        case .film:    return "Film"
        case .oav:     return "OAV"
        case .special: return "Spécial"
        case .extra:   return "Bonus"
        }
    }
}

/// The result of parsing a filename, before any network lookup.
struct ParsedName: Hashable, Codable {
    var title: String
    var kind: MediaKind
    var year: Int?
    var season: Int?
    var episode: Int?
    /// Anime-style absolute numbering (e.g. "Nicky Larson - 001") where no season is given.
    var absoluteEpisode: Int?
    var releaseGroup: String?
    /// Set when the file is a film/OAV/special rather than a numbered episode.
    var special: SpecialKind?
    /// The special's own title, taken from the filename ("Bay City Wars").
    var specialTitle: String?
    var specialNumber: Int?

    /// Stable key used for caching lookups, so all episodes of one show share a show lookup.
    var showKey: String {
        var key = title.lowercased()
        if let year { key += "|\(year)" }
        key += "|\(kind.rawValue)"
        return key
    }
}

/// One possible match, offered to the user when the automatic ranking is not
/// trustworthy. "Pusher 2" scores no better against *Pusher II* than against
/// *Layer Cake* -- the catalogue spells the sequel in Roman numerals, so no
/// candidate matches on title and popularity alone decides. Letting the user
/// point at the right row settles it for good.
struct MatchCandidate: Identifiable, Hashable, Sendable {
    /// The provider's own id, unique only within its kind.
    let providerID: Int
    let isSeries: Bool
    let title: String
    let originalTitle: String?
    let year: Int?
    let overview: String?
    let posterURL: String?

    var id: String { "\(isSeries ? "tv" : "movie")-\(providerID)" }

    /// "2004 · Pusher II" -- year first, then the original title when the
    /// localised one differs, which is exactly what tells two sequels apart.
    var subtitle: String {
        var parts: [String] = []
        if let year { parts.append(String(year)) }
        if let originalTitle, originalTitle != title { parts.append(originalTitle) }
        return parts.joined(separator: " · ")
    }
}

/// Metadata resolved from an online provider.
struct MediaMetadata: Codable, Hashable {
    var seriesTitle: String?
    var episodeTitle: String?
    var overview: String?
    var year: Int?
    var season: Int?
    var episode: Int?
    var rating: Double?
    var genres: [String] = []
    var posterURL: String?
    var stillURL: String?
    var source: String?
    /// True when the provider matched something; false records a confirmed miss so we don't retry forever.
    var matched: Bool = true

    /// "S01E01 · One Cool Sweeper" style subtitle.
    var episodeCode: String? {
        guard let season, let episode else { return nil }
        return String(format: "S%02dE%02d", season, episode)
    }
}

/// One scanned file plus everything we know about it.
struct MediaItem: Identifiable, Hashable {
    var id: String { url.path }
    let url: URL
    let parsed: ParsedName
    let fileSize: Int64
    let modified: Date
    var metadata: MediaMetadata?
    var lookupFailed: Bool = false

    var filename: String { url.lastPathComponent }

    /// Best title we can show: resolved metadata first, parsed filename second.
    var displayTitle: String {
        if let m = metadata, m.matched {
            if parsed.kind == .episode, let ep = m.episodeTitle, !ep.isEmpty { return ep }
            if let s = m.seriesTitle, !s.isEmpty { return s }
        }
        return parsed.title
    }

    /// Secondary line under the title.
    var displaySubtitle: String {
        var parts: [String] = []
        if parsed.kind == .episode {
            if let m = metadata, m.matched, let series = m.seriesTitle { parts.append(series) }
            if let code = metadata?.episodeCode {
                parts.append(code)
            } else if let abs = parsed.absoluteEpisode {
                parts.append(String(format: "Ép. %03d", abs))
            } else if let s = parsed.season, let e = parsed.episode {
                parts.append(String(format: "S%02dE%02d", s, e))
            }
        } else {
            if let y = metadata?.year ?? parsed.year { parts.append(String(y)) }
            if let g = metadata?.genres.first { parts.append(g) }
        }
        if parts.isEmpty { parts.append(url.pathExtension.uppercased()) }
        return parts.joined(separator: " · ")
    }

    var kind: MediaKind { parsed.kind }

    var formattedSize: String {
        ByteCountFormatter.string(fromByteCount: fileSize, countStyle: .file)
    }

    /// Resolved season number, preferring what the provider confirmed.
    var seasonNumber: Int? { metadata?.season ?? parsed.season }

    /// Resolved episode number, falling back to absolute numbering.
    var episodeNumber: Int? {
        metadata?.episode ?? parsed.episode ?? parsed.absoluteEpisode
    }

    /// Title for one row of a season's episode list.
    var episodeRowTitle: String {
        if let special = parsed.specialTitle, !special.isEmpty { return special }
        if let m = metadata, m.matched, let ep = m.episodeTitle, !ep.isEmpty { return ep }
        return url.deletingPathExtension().lastPathComponent
    }

    var special: SpecialKind? { parsed.special }

    /// Heading shown over the video. Falls back through metadata rather than
    /// exposing the raw release filename.
    var playerTitle: String {
        if let special = parsed.specialTitle, !special.isEmpty { return special }
        if kind == .episode, let m = metadata, m.matched,
           let ep = m.episodeTitle, !ep.isEmpty { return ep }
        if let m = metadata, m.matched, let s = m.seriesTitle, !s.isEmpty { return s }
        return parsed.title
    }

    /// Second line over the video: series and episode code, or year and genre.
    var playerSubtitle: String {
        var parts: [String] = []
        if kind == .episode {
            if let series = metadata?.seriesTitle, metadata?.matched == true { parts.append(series) }
            if let code = metadata?.episodeCode { parts.append(code) }
            else if let abs = parsed.absoluteEpisode { parts.append(String(format: "Ép. %03d", abs)) }
            if let special = parsed.special { parts.append(special.rowLabel) }
        } else {
            if let y = metadata?.year ?? parsed.year { parts.append(String(y)) }
            if let g = metadata?.genres.first { parts.append(g) }
        }
        return parts.joined(separator: " · ")
    }

    /// Series name used to group episodes in the sidebar / grid sections.
    var groupingTitle: String {
        if parsed.kind == .episode {
            if let m = metadata, m.matched, let s = m.seriesTitle, !s.isEmpty { return s }
            return parsed.title
        }
        return displayTitle
    }
}
