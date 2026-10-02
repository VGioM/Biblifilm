import Foundation

/// themoviedb.org. Best coverage: knows localized/alternative titles
/// (e.g. "Nicky Larson" -> City Hunter) and has episode names per language.
struct TMDBProvider: MetadataProvider {
    let name = "TMDB"
    let apiKey: String

    private static let base = "https://api.themoviedb.org/3"
    private static let imageBase = "https://image.tmdb.org/t/p"

    /// TMDB's settings page offers two credentials: the short v3 "API Key" and a
    /// long v4 "Read Access Token" (a JWT). People paste either, so support both.
    private var isBearerToken: Bool {
        apiKey.hasPrefix("eyJ") && apiKey.contains(".")
    }

    private var authHeaders: [String: String] {
        isBearerToken ? ["Authorization": "Bearer \(apiKey)"] : [:]
    }

    // MARK: - Response shapes

    private struct SearchResponse<T: Decodable>: Decodable { let results: [T] }

    private struct TVResult: Decodable {
        let id: Int
        let name: String?
        let original_name: String?
        let overview: String?
        let first_air_date: String?
        let poster_path: String?
        let vote_average: Double?
        let popularity: Double?
    }

    private struct MovieResult: Decodable {
        let id: Int
        let title: String?
        let original_title: String?
        let overview: String?
        let release_date: String?
        let poster_path: String?
        let vote_average: Double?
        let popularity: Double?
    }

    private struct Genre: Decodable { let name: String }
    private struct MovieDetail: Decodable {
        let title: String?
        let original_title: String?
        let overview: String?
        let release_date: String?
        let poster_path: String?
        let vote_average: Double?
        let genres: [Genre]?
        let runtime: Int?
    }

    private struct SeasonSummary: Decodable {
        let season_number: Int
        let episode_count: Int
    }
    private struct TVDetail: Decodable {
        let name: String?
        let overview: String?
        let first_air_date: String?
        let poster_path: String?
        let vote_average: Double?
        let genres: [Genre]?
        let seasons: [SeasonSummary]?
    }

    private struct EpisodeDetail: Decodable {
        let name: String?
        let overview: String?
        let still_path: String?
        let season_number: Int?
        let episode_number: Int?
        let vote_average: Double?
    }

    // MARK: - Lookup

    func lookup(_ parsed: ParsedName, language: String) async throws -> MediaMetadata? {
        guard !apiKey.isEmpty else { throw MetadataError.noKey }
        switch parsed.kind {
        case .episode:            return try await lookupTV(parsed, language: language)
        case .movie, .other:      return try await lookupMovie(parsed, language: language)
        case .audio:             return nil
        }
    }

    private func url(_ path: String, _ query: [String: String]) -> URL {
        var comps = URLComponents(string: Self.base + path)!
        var items: [URLQueryItem] = isBearerToken ? [] : [URLQueryItem(name: "api_key", value: apiKey)]
        items += query.map { URLQueryItem(name: $0.key, value: $0.value) }
        comps.queryItems = items
        return comps.url!
    }

    /// Convenience wrapper so every call carries the right auth style.
    private func get<T: Decodable>(_ type: T.Type, _ path: String,
                                   _ query: [String: String]) async throws -> T {
        try await HTTP.json(type, from: url(path, query), headers: authHeaders)
    }

    /// Checks a key against TMDB so Settings can confirm it works.
    /// Returns nil on success, or a human-readable reason.
    static func validate(key: String) async -> String? {
        let probe = TMDBProvider(apiKey: key.trimmingCharacters(in: .whitespacesAndNewlines))
        guard !probe.apiKey.isEmpty else { return "Aucune clé saisie." }
        struct Config: Decodable { let images: Images?; struct Images: Decodable { let secure_base_url: String? } }
        do {
            _ = try await probe.get(Config.self, "/configuration", [:])
            return nil
        } catch MetadataError.badResponse(let code) {
            if code == 401 {
                return probe.isBearerToken
                    ? "Jeton refusé (401). Vérifiez le « API Read Access Token »."
                    : "Clé refusée (401). Copiez la « API Key » (32 caractères) ou le jeton de lecture."
            }
            return "TMDB a répondu HTTP \(code)."
        } catch {
            return "Connexion impossible : \(error.localizedDescription)"
        }
    }

    private func image(_ path: String?, size: String) -> String? {
        guard let path, !path.isEmpty else { return nil }
        return "\(Self.imageBase)/\(size)\(path)"
    }

    /// Lowercased, unaccented, punctuation-free form used to compare titles.
    private func normalize(_ s: String?) -> String {
        guard let s else { return "" }
        return s.folding(options: [.diacriticInsensitive, .caseInsensitive], locale: nil)
            .replacingOccurrences(of: #"[^a-z0-9 ]"#, with: " ", options: .regularExpression)
            .replacingOccurrences(of: #"\s+"#, with: " ", options: .regularExpression)
            .trimmingCharacters(in: .whitespaces)
    }

    /// TMDB's own ordering is unreliable for exact titles -- searching
    /// "Independence Day" puts *Resurgence* above the 1996 original, and
    /// "Monster" returns "Monster House" first. Rank on exact-title and year
    /// agreement, using popularity only to break ties.
    private func score(title: String?, originalTitle: String?, date: String?,
                       popularity: Double?, want: ParsedName) -> Double {
        var score = 0.0
        let wanted = normalize(want.title)
        if !wanted.isEmpty {
            if normalize(title) == wanted || normalize(originalTitle) == wanted {
                score += 100
            } else if normalize(title).hasPrefix(wanted) || normalize(originalTitle).hasPrefix(wanted) {
                score += 25
            }
        }
        if let wantYear = want.year, let got = year(from: date) {
            let gap = abs(got - wantYear)
            if gap == 0 { score += 60 }
            else if gap <= 1 { score += 30 }
            else { score -= 20 }
        }
        score += min((popularity ?? 0) / 10.0, 5)   // gentle tiebreak only
        return score
    }

    private func year(from date: String?) -> Int? {
        guard let date, date.count >= 4 else { return nil }
        return Int(date.prefix(4))
    }

    /// True when a candidate's own title backs up the match, rather than the
    /// release year alone agreeing by chance.
    private func titleAgrees(_ title: String?, _ originalTitle: String?,
                             with want: ParsedName) -> Bool {
        let wanted = normalize(want.title)
        guard !wanted.isEmpty else { return false }
        return normalize(title) == wanted || normalize(originalTitle) == wanted
            || normalize(title).hasPrefix(wanted) || normalize(originalTitle).hasPrefix(wanted)
    }

    /// Collections are numbered by their owner, not by the catalogue: the
    /// first film of a series is filed as "Rocky", never "Rocky I". Searching
    /// the numbered form returns two obscure 1976 shorts and no Rocky at all,
    /// so the trailing numeral is worth dropping -- but only as a fallback,
    /// after the real title failed, so "Star Wars Episode I" is left alone.
    private func withoutFirstInstalmentNumber(_ title: String) -> String? {
        let trimmed = title.replacingOccurrences(
            of: #"\s+(?:I|1)$"#, with: "", options: [.regularExpression])
        guard trimmed != title, trimmed.count >= 2 else { return nil }
        return trimmed
    }

    // MARK: - Manual pick

    /// Every plausible match, best guess first, for the user to choose from.
    ///
    /// Ranked exactly like the automatic pick, so the top row is what the app
    /// settled on by itself and the rows under it are the ones it passed over --
    /// which is the whole point when the ranking got it wrong.
    func candidates(_ parsed: ParsedName, language: String) async throws -> [MatchCandidate] {
        guard !apiKey.isEmpty else { throw MetadataError.noKey }
        let wantsSeries = parsed.kind == .episode

        if wantsSeries {
            var query = ["query": parsed.title, "language": language, "include_adult": "false"]
            if let y = parsed.year { query["first_air_date_year"] = String(y) }
            var results = try await get(SearchResponse<TVResult>.self, "/search/tv", query).results
            if results.isEmpty, parsed.year != nil {
                query.removeValue(forKey: "first_air_date_year")
                results = try await get(SearchResponse<TVResult>.self, "/search/tv", query).results
            }
            return results
                .sorted {
                    score(title: $0.name, originalTitle: $0.original_name,
                          date: $0.first_air_date, popularity: $0.popularity, want: parsed)
                    > score(title: $1.name, originalTitle: $1.original_name,
                            date: $1.first_air_date, popularity: $1.popularity, want: parsed)
                }
                .prefix(12)
                .map {
                    MatchCandidate(
                        providerID: $0.id, isSeries: true,
                        title: $0.name ?? $0.original_name ?? parsed.title,
                        originalTitle: $0.original_name,
                        year: year(from: $0.first_air_date),
                        overview: $0.overview,
                        posterURL: image($0.poster_path, size: "w342"))
                }
        }

        var query = ["query": parsed.title, "language": language, "include_adult": "false"]
        if let y = parsed.year { query["year"] = String(y) }
        var results = try await get(SearchResponse<MovieResult>.self, "/search/movie", query).results

        // The year is a filter, not a fact: a mislabelled file deserves the
        // wider net rather than an empty list.
        if results.count < 2, parsed.year != nil {
            query.removeValue(forKey: "year")
            let wider = try await get(SearchResponse<MovieResult>.self, "/search/movie", query).results
            let known = Set(results.map(\.id))
            results += wider.filter { !known.contains($0.id) }
        }

        return results
            .sorted {
                score(title: $0.title, originalTitle: $0.original_title,
                      date: $0.release_date, popularity: $0.popularity, want: parsed)
                > score(title: $1.title, originalTitle: $1.original_title,
                        date: $1.release_date, popularity: $1.popularity, want: parsed)
            }
            .prefix(12)
            .map {
                MatchCandidate(
                    providerID: $0.id, isSeries: false,
                    title: $0.title ?? $0.original_title ?? parsed.title,
                    originalTitle: $0.original_title,
                    year: year(from: $0.release_date),
                    overview: $0.overview,
                    posterURL: image($0.poster_path, size: "w342"))
            }
    }

    /// Full metadata for a candidate the user picked, no ranking involved.
    func metadata(for candidate: MatchCandidate, parsed: ParsedName,
                  language: String) async throws -> MediaMetadata? {
        guard !apiKey.isEmpty else { throw MetadataError.noKey }
        if candidate.isSeries {
            return try await tvMetadata(showID: candidate.providerID, parsed: parsed,
                                        language: language, fallback: nil)
        }
        return try await movieMetadata(id: candidate.providerID, language: language, fallback: nil)
    }

    // MARK: Movies

    private func lookupMovie(_ parsed: ParsedName, language: String) async throws -> MediaMetadata? {
        var query: [String: String] = ["query": parsed.title, "language": language,
                                       "include_adult": "false"]
        if let y = parsed.year { query["year"] = String(y) }

        var results = try await get(SearchResponse<MovieResult>.self, "/search/movie", query).results

        // Retry without the year filter if it was too strict.
        if results.isEmpty, parsed.year != nil {
            query.removeValue(forKey: "year")
            results = try await get(SearchResponse<MovieResult>.self, "/search/movie", query).results
        }
        var best = pickMovie(results, want: parsed)

        // Nothing whose title actually matches: try the un-numbered title
        // before settling for whatever shared the year.
        if best == nil || !titleAgrees(best?.title, best?.original_title, with: parsed),
           let variant = withoutFirstInstalmentNumber(parsed.title) {
            var retry = parsed
            retry.title = variant
            var altQuery: [String: String] = ["query": variant, "language": language,
                                              "include_adult": "false"]
            if let y = parsed.year { altQuery["year"] = String(y) }
            var alt = try await get(SearchResponse<MovieResult>.self, "/search/movie", altQuery).results
            if alt.isEmpty, parsed.year != nil {
                altQuery.removeValue(forKey: "year")
                alt = try await get(SearchResponse<MovieResult>.self, "/search/movie", altQuery).results
            }
            if let candidate = pickMovie(alt, want: retry),
               titleAgrees(candidate.title, candidate.original_title, with: retry) {
                best = candidate
            }
        }
        guard let best else { return nil }
        return try await movieMetadata(id: best.id, language: language, fallback: best)
    }

    /// Everything known about one movie, by id. The search result is kept as a
    /// fallback because a localised detail record can come back with an empty
    /// overview; a manual pick passes none and relies on the detail alone.
    private func movieMetadata(id: Int, language: String,
                               fallback: MovieResult?) async throws -> MediaMetadata? {
        let detail = try? await get(MovieDetail.self, "/movie/\(id)", ["language": language])
        guard detail != nil || fallback != nil else { return nil }

        let overview = [detail?.overview, fallback?.overview]
            .compactMap { $0 }
            .first { !$0.isEmpty }

        return MediaMetadata(
            seriesTitle: detail?.title ?? fallback?.title
                ?? detail?.original_title ?? fallback?.original_title,
            episodeTitle: nil,
            overview: overview,
            year: year(from: detail?.release_date ?? fallback?.release_date),
            season: nil, episode: nil,
            rating: detail?.vote_average ?? fallback?.vote_average,
            genres: detail?.genres?.map(\.name) ?? [],
            posterURL: image(detail?.poster_path ?? fallback?.poster_path, size: "w500"),
            stillURL: nil,
            source: name
        )
    }

    private func pickMovie(_ results: [MovieResult], want: ParsedName) -> MovieResult? {
        results.max(by: {
            score(title: $0.title, originalTitle: $0.original_title,
                  date: $0.release_date, popularity: $0.popularity, want: want)
            < score(title: $1.title, originalTitle: $1.original_title,
                    date: $1.release_date, popularity: $1.popularity, want: want)
        })
    }

    // MARK: TV

    private func lookupTV(_ parsed: ParsedName, language: String) async throws -> MediaMetadata? {
        var tvQuery = ["query": parsed.title, "language": language, "include_adult": "false"]
        if let y = parsed.year { tvQuery["first_air_date_year"] = String(y) }

        var results = try await get(SearchResponse<TVResult>.self, "/search/tv", tvQuery).results
        if results.isEmpty, parsed.year != nil {
            tvQuery.removeValue(forKey: "first_air_date_year")
            results = try await get(SearchResponse<TVResult>.self, "/search/tv", tvQuery).results
        }
        guard let show = results.max(by: {
            score(title: $0.name, originalTitle: $0.original_name,
                  date: $0.first_air_date, popularity: $0.popularity, want: parsed)
            < score(title: $1.name, originalTitle: $1.original_name,
                    date: $1.first_air_date, popularity: $1.popularity, want: parsed)
        }) else { return nil }

        return try await tvMetadata(showID: show.id, parsed: parsed,
                                    language: language, fallback: show)
    }

    /// Everything known about one episode of a given show. Split out from the
    /// search so a manually chosen series can be resolved the same way.
    private func tvMetadata(showID: Int, parsed: ParsedName, language: String,
                            fallback: TVResult?) async throws -> MediaMetadata? {
        let detail = try? await get(TVDetail.self, "/tv/\(showID)", ["language": language])
        guard detail != nil || fallback != nil else { return nil }

        let seriesTitle = detail?.name ?? fallback?.name ?? fallback?.original_name
        let poster = image(detail?.poster_path ?? fallback?.poster_path, size: "w500")

        // Resolve which season/episode to ask for.
        var season = parsed.season
        var episode = parsed.episode
        if season == nil, let abs = parsed.absoluteEpisode {
            (season, episode) = resolveAbsolute(abs, seasons: detail?.seasons)
        }

        guard let s = season, let e = episode else {
            // Show matched but we can't pin the episode -- still worth showing the series.
            return MediaMetadata(
                seriesTitle: seriesTitle, episodeTitle: nil,
                overview: detail?.overview ?? fallback?.overview,
                year: year(from: detail?.first_air_date ?? fallback?.first_air_date),
                season: nil, episode: nil,
                rating: detail?.vote_average ?? fallback?.vote_average,
                genres: detail?.genres?.map(\.name) ?? [],
                posterURL: poster, stillURL: nil, source: name)
        }

        let ep = try? await get(EpisodeDetail.self,
                                "/tv/\(showID)/season/\(s)/episode/\(e)", ["language": language])

        return MediaMetadata(
            seriesTitle: seriesTitle,
            episodeTitle: ep?.name,
            overview: (ep?.overview?.isEmpty == false ? ep?.overview : nil)
                        ?? detail?.overview ?? fallback?.overview,
            year: year(from: detail?.first_air_date ?? fallback?.first_air_date),
            season: ep?.season_number ?? s,
            episode: ep?.episode_number ?? e,
            rating: ep?.vote_average ?? detail?.vote_average,
            genres: detail?.genres?.map(\.name) ?? [],
            posterURL: poster,
            stillURL: image(ep?.still_path, size: "w780"),
            source: name)
    }

    /// Map an absolute episode number (anime style) onto season/episode by
    /// walking the season episode counts in order.
    private func resolveAbsolute(_ absolute: Int, seasons: [SeasonSummary]?) -> (Int?, Int?) {
        guard let seasons, !seasons.isEmpty else { return (1, absolute) }
        var remaining = absolute
        for season in seasons.filter({ $0.season_number > 0 })
                             .sorted(by: { $0.season_number < $1.season_number }) {
            if remaining <= season.episode_count {
                return (season.season_number, remaining)
            }
            remaining -= season.episode_count
        }
        return (1, absolute)   // beyond known seasons: assume flat numbering
    }
}
