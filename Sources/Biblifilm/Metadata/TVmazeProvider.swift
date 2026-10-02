import Foundation

/// Keyless fallback: TVmaze for shows, with Wikidata used to translate a
/// localized title into the name TVmaze indexes it under
/// (e.g. "Nicky Larson" -> "City Hunter").
struct TVmazeProvider: MetadataProvider {
    let name = "TVmaze"

    // MARK: - Response shapes

    private struct SearchHit: Decodable {
        let score: Double
        let show: Show
    }
    private struct Show: Decodable {
        let id: Int
        let name: String
        let premiered: String?
        let summary: String?
        let genres: [String]?
        let image: Img?
        let rating: Rating?
        let type: String?
        let language: String?

        /// Absolute episode numbering is an anime convention, so this flag
        /// breaks ties between same-named live-action and animated shows
        /// (e.g. City Hunter 2011 Korean drama vs. the 1987 anime).
        var looksAnimated: Bool {
            if type?.caseInsensitiveCompare("Animation") == .orderedSame { return true }
            return genres?.contains { $0.caseInsensitiveCompare("Anime") == .orderedSame } ?? false
        }
    }
    private struct Img: Decodable { let medium: String?; let original: String? }
    private struct Rating: Decodable { let average: Double? }
    private struct Episode: Decodable {
        let name: String?
        let season: Int?
        let number: Int?
        let summary: String?
        let image: Img?
    }
    private struct WikidataSearch: Decodable {
        struct Entity: Decodable { let id: String; let label: String? }
        let search: [Entity]
    }

    /// Below this TVmaze score the match is noise.
    private static let minimumScore = 0.6

    func lookup(_ parsed: ParsedName, language: String) async throws -> MediaMetadata? {
        // TVmaze only knows television. Searching it for a film returns
        // confident-looking nonsense ("Black Hawk Down" -> the documentary
        // series "Surviving Black Hawk Down"), which is worse than no match,
        // so films are left to TMDB.
        guard parsed.kind == .episode else { return nil }

        var show = try await searchShow(parsed.title, parsed: parsed)

        // No confident hit: the title may be localized. Ask Wikidata for the
        // canonical (usually English) label and retry.
        if show == nil, let canonical = try? await canonicalTitle(for: parsed.title),
           canonical.lowercased() != parsed.title.lowercased() {
            show = try await searchShow(canonical, parsed: parsed)
        }
        guard let show else { return nil }

        var meta = MediaMetadata(
            seriesTitle: show.name,
            episodeTitle: nil,
            overview: strip(show.summary),
            year: show.premiered.flatMap { Int($0.prefix(4)) },
            rating: show.rating?.average,
            genres: show.genres ?? [],
            posterURL: show.image?.original ?? show.image?.medium,
            source: name)

        if let ep = try? await episode(showID: show.id, parsed: parsed) {
            meta.episodeTitle = ep.name
            meta.season = ep.season
            meta.episode = ep.number
            if let s = strip(ep.summary), !s.isEmpty { meta.overview = s }
            meta.stillURL = ep.image?.original ?? ep.image?.medium
        }
        return meta
    }

    private func searchShow(_ title: String, parsed: ParsedName) async throws -> Show? {
        let url = URL(string: "https://api.tvmaze.com/search/shows?q=\(HTTP.encode(title))")!
        let hits = try await HTTP.json([SearchHit].self, from: url)
        let viable = hits.filter { $0.score >= Self.minimumScore }
        guard !viable.isEmpty else { return nil }

        // Many shows share a name ("Monster" is a 2004 anime and a 2016 Korean
        // drama), so rank on more than TVmaze's text score.
        let preferAnimated = parsed.absoluteEpisode != nil
        let ranked = viable.max { a, b in
            rank(a, wantYear: parsed.year, preferAnimated: preferAnimated)
                < rank(b, wantYear: parsed.year, preferAnimated: preferAnimated)
        }
        return ranked?.show
    }

    /// Text score, plus a strong bonus when the premiere year matches the
    /// filename's year, plus a nudge toward animation for absolute numbering.
    private func rank(_ hit: SearchHit, wantYear: Int?, preferAnimated: Bool) -> Double {
        var score = hit.score
        if let wantYear, let premiered = hit.show.premiered,
           let year = Int(premiered.prefix(4)) {
            let gap = abs(year - wantYear)
            if gap == 0 { score += 1.0 }
            else if gap <= 1 { score += 0.5 }
            else if gap >= 10 { score -= 0.3 }
        }
        if preferAnimated && hit.show.looksAnimated { score += 0.4 }
        return score
    }

    /// Wikidata knows localized titles and maps them to a canonical label.
    private func canonicalTitle(for title: String) async throws -> String? {
        let url = URL(string:
            "https://www.wikidata.org/w/api.php?action=wbsearchentities" +
            "&search=\(HTTP.encode(title))&language=fr&uselang=en&format=json&limit=3&type=item")!
        let res = try await HTTP.json(WikidataSearch.self, from: url)
        return res.search.first?.label
    }

    private func episode(showID: Int, parsed: ParsedName) async throws -> Episode? {
        if let s = parsed.season, let e = parsed.episode {
            let url = URL(string:
                "https://api.tvmaze.com/shows/\(showID)/episodebynumber?season=\(s)&number=\(e)")!
            return try? await HTTP.json(Episode.self, from: url)
        }
        if let abs = parsed.absoluteEpisode {
            // TVmaze has no absolute lookup: index into the full ordered list.
            let url = URL(string: "https://api.tvmaze.com/shows/\(showID)/episodes")!
            let all = try await HTTP.json([Episode].self, from: url)
            guard abs >= 1, abs <= all.count else { return nil }
            return all[abs - 1]
        }
        return nil
    }

    /// TVmaze summaries are HTML fragments.
    private func strip(_ html: String?) -> String? {
        guard let html else { return nil }
        let text = html
            .replacingOccurrences(of: "<[^>]+>", with: "", options: .regularExpression)
            .replacingOccurrences(of: "&amp;", with: "&")
            .replacingOccurrences(of: "&quot;", with: "\"")
            .replacingOccurrences(of: "&#39;", with: "'")
            .replacingOccurrences(of: "&nbsp;", with: " ")
            .trimmingCharacters(in: .whitespacesAndNewlines)
        return text.isEmpty ? nil : text
    }
}
