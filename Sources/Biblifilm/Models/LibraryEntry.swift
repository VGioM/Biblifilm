import Foundation

/// Every episode of one season of one series, shown in the grid as a single tile.
struct SeasonGroup: Identifiable, Hashable {
    let seriesTitle: String
    let season: Int?
    /// Set when this tile holds films/OAVs/bonus rather than numbered episodes.
    var special: SpecialKind?
    var episodes: [MediaItem]

    var id: String {
        if let special { return "\(seriesTitle)|x\(special.bucketLabel)" }
        return "\(seriesTitle)|s\(season.map(String.init) ?? "?")"
    }

    /// The first episode that actually resolved artwork wins.
    var posterURL: String? {
        episodes.compactMap { $0.metadata?.posterURL }.first
    }

    var year: Int? { episodes.compactMap { $0.metadata?.year }.first }
    var genres: [String] { episodes.compactMap { $0.metadata?.genres }.first(where: { !$0.isEmpty }) ?? [] }
    var overview: String? {
        episodes.compactMap { $0.metadata?.overview }.first { !($0.isEmpty) }
    }

    var isIdentified: Bool { episodes.contains { $0.metadata?.matched == true } }
    var totalSize: Int64 { episodes.reduce(0) { $0 + $1.fileSize } }
    var latestModified: Date { episodes.map(\.modified).max() ?? .distantPast }

    var seasonLabel: String {
        if let special { return special.bucketLabel }
        guard let season else { return "Épisodes" }
        return "Saison \(season)"
    }

    var subtitle: String {
        let n = episodes.count
        if special != nil {
            return "\(seasonLabel) · \(n) titre\(n > 1 ? "s" : "")"
        }
        return "\(seasonLabel) · \(n) épisode\(n > 1 ? "s" : "")"
    }

    var formattedSize: String {
        ByteCountFormatter.string(fromByteCount: totalSize, countStyle: .file)
    }

    /// Episodes in playback order; specials by their own numbering.
    static func sortEpisodes(_ items: [MediaItem]) -> [MediaItem] {
        items.sorted { lhs, rhs in
            let l = (lhs.parsed.special != nil ? lhs.parsed.specialNumber : lhs.episodeNumber) ?? Int.max
            let r = (rhs.parsed.special != nil ? rhs.parsed.specialNumber : rhs.episodeNumber) ?? Int.max
            return (l, lhs.filename) < (r, rhs.filename)
        }
    }
}

/// One tile in the grid: a standalone file, or a whole season.
enum LibraryEntry: Identifiable, Hashable {
    case single(MediaItem)
    case season(SeasonGroup)

    var id: String {
        switch self {
        case .single(let item):  return "f:" + item.id
        case .season(let group): return "s:" + group.id
        }
    }

    /// Sorting and display both key off the series/film title, never the episode title.
    var sortTitle: String {
        switch self {
        case .single(let item):  return item.groupingTitle
        case .season(let group): return group.seriesTitle
        }
    }

    /// Ordering within one series: seasons first, then films, then bonus.
    var seasonNumber: Int {
        switch self {
        case .single:
            return 0
        case .season(let group):
            switch group.special {
            case .none:              return group.season ?? 9_000
            case .film, .oav, .special: return 10_000
            case .extra:             return 10_001
            }
        }
    }

    var displayTitle: String { sortTitle }

    var displaySubtitle: String {
        switch self {
        case .single(let item):  return item.displaySubtitle
        case .season(let group): return group.subtitle
        }
    }

    var posterURL: String? {
        switch self {
        case .single(let item):  return item.metadata?.posterURL
        case .season(let group): return group.posterURL
        }
    }

    var kind: MediaKind {
        switch self {
        case .single(let item): return item.kind
        case .season:           return .episode
        }
    }

    var latestModified: Date {
        switch self {
        case .single(let item):  return item.modified
        case .season(let group): return group.latestModified
        }
    }

    var totalSize: Int64 {
        switch self {
        case .single(let item):  return item.fileSize
        case .season(let group): return group.totalSize
        }
    }

    var episodeCount: Int {
        switch self {
        case .single:            return 1
        case .season(let group): return group.episodes.count
        }
    }

    var isIdentified: Bool {
        switch self {
        case .single(let item):  return item.metadata?.matched == true
        case .season(let group): return group.isIdentified
        }
    }

    /// Everything playable in this tile, in order.
    var playableURLs: [URL] {
        switch self {
        case .single(let item):  return [item.url]
        case .season(let group): return group.episodes.map(\.url)
        }
    }

    /// Groups episodes by series + season; everything else stays standalone.
    static func build(from items: [MediaItem]) -> [LibraryEntry] {
        var singles: [LibraryEntry] = []
        var buckets: [String: SeasonGroup] = [:]
        var order: [String] = []

        for item in items {
            guard item.kind == .episode else {
                singles.append(.single(item))
                continue
            }
            // Films, OAVs and bonus material get their own tile rather than
            // being mixed into the season's episode list.
            let key: String
            if let special = item.parsed.special {
                key = "\(item.groupingTitle)|x\(special.bucketLabel)"
            } else {
                key = "\(item.groupingTitle)|s\(item.seasonNumber.map(String.init) ?? "?")"
            }
            if buckets[key] == nil {
                buckets[key] = SeasonGroup(
                    seriesTitle: item.groupingTitle,
                    season: item.parsed.special == nil ? item.seasonNumber : nil,
                    special: item.parsed.special,
                    episodes: [])
                order.append(key)
            }
            buckets[key]?.episodes.append(item)
        }

        var result = singles
        for key in order {
            guard var group = buckets[key] else { continue }
            group.episodes = SeasonGroup.sortEpisodes(group.episodes)
            result.append(.season(group))
        }
        return result
    }
}
