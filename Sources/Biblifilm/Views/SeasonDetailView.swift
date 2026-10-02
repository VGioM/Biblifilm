import SwiftUI

/// A season's page: cover and summary on top, then every episode as a row you can play.
struct SeasonDetailView: View {
    @ObservedObject var model: LibraryModel
    let group: SeasonGroup

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 18) {
                header
                Divider().opacity(0.5)
                episodeList
            }
            .padding(22)
        }
        .background(Theme.contentBackground)
    }

    /// "Reprendre l'épisode 4" when something is part-watched.
    private var seasonButtonTitle: String {
        let store = WatchStore.shared
        if let next = store.nextEpisode(in: group),
           store.resumePosition(for: next) != nil {
            return "Reprendre l'épisode \(next.episodeNumber ?? 1)"
        }
        if store.watchedCount(in: group) > 0,
           let next = store.nextEpisode(in: group) {
            return "Lire l'épisode \(next.episodeNumber ?? 1)"
        }
        return "Lire la saison"
    }

    private var header: some View {
        HStack(alignment: .top, spacing: 18) {
            ArtworkView(
                urlString: group.posterURL,
                seed: group.seriesTitle,
                symbol: "tv",
                aspect: Theme.posterAspect,
                cornerRadius: 10)
            .frame(width: 168)
            .shadow(color: .black.opacity(0.45), radius: 12, y: 5)

            VStack(alignment: .leading, spacing: 7) {
                Text(group.seriesTitle)
                    .font(.system(size: 22, weight: .bold))
                    .fixedSize(horizontal: false, vertical: true)

                Text(group.subtitle)
                    .font(.title3)
                    .foregroundStyle(Theme.accent)

                HStack(spacing: 6) {
                    if let y = group.year { Text(String(y)) }
                    Text(group.formattedSize)
                }
                .font(.subheadline)
                .foregroundStyle(.secondary)

                if !group.genres.isEmpty { FlowTags(tags: group.genres) }

                if let overview = group.overview, !overview.isEmpty {
                    Text(overview)
                        .font(.callout)
                        .foregroundStyle(.primary.opacity(0.85))
                        .lineLimit(4)
                        .fixedSize(horizontal: false, vertical: true)
                        .padding(.top, 2)
                }

                HStack(spacing: 8) {
                    Button {
                        model.play(.season(group))
                    } label: {
                        Label(seasonButtonTitle, systemImage: "play.fill")
                    }
                    .buttonStyle(.borderedProminent)
                    .tint(Theme.accent)
                    .controlSize(.large)

                    Menu {
                        Button("Marquer la saison comme vue") {
                            WatchStore.shared.setWatched(true, for: group.episodes)
                        }
                        Button("Marquer comme non vue") {
                            WatchStore.shared.setWatched(false, for: group.episodes)
                        }
                    } label: {
                        Image(systemName: "ellipsis.circle")
                    }
                    .menuStyle(.borderlessButton)
                    .frame(width: 34)
                }
                .padding(.top, 4)

                if !group.isIdentified {
                    Text("Série non identifiée — ajoutez une clé TMDB dans les réglages, ou utilisez « Corriger » sur un épisode.")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
            Spacer(minLength: 0)
        }
    }

    private var episodeList: some View {
        VStack(spacing: 0) {
            ForEach(Array(group.episodes.enumerated()), id: \.element.id) { index, episode in
                EpisodeRow(
                    episode: episode,
                    fallbackNumber: index + 1,
                    isSelected: model.selectedItemID == episode.id,
                    onPlay: { model.play(episode) },
                    onSelect: { model.select(episode.id) })
                .contextMenu {
                    Button("Lire dans VLC") { model.play(episode) }
                    if WatchStore.shared.resumePosition(for: episode) != nil {
                        Button("Lire depuis le début") { model.play(episode, fromStart: true) }
                    }
                    Button(WatchStore.shared.isWatched(episode)
                           ? "Marquer comme non vu" : "Marquer comme vu") {
                        WatchStore.shared.setWatched(!WatchStore.shared.isWatched(episode),
                                                     for: episode)
                    }
                    Divider()
                    Button("Ouvrir dans VLC") { model.openExternally(episode) }
                    Button("Afficher dans le Finder") { model.reveal(episode) }
                    Divider()
                    Button("Rechercher les infos à nouveau") { model.refetch(episode) }
                }
                if episode.id != group.episodes.last?.id {
                    Divider().opacity(0.28).padding(.leading, 44)
                }
            }
        }
    }
}

/// One episode line: number, title, size, and a play button on hover.
struct EpisodeRow: View {
    let episode: MediaItem
    let fallbackNumber: Int
    let isSelected: Bool
    let onPlay: () -> Void
    let onSelect: () -> Void

    @ObservedObject private var watch = WatchStore.shared
    @State private var isHovering = false

    private var isWatched: Bool { watch.isWatched(episode) }
    private var resume: Double? { watch.resumePosition(for: episode) }

    private func remaining(_ r: WatchRecord) -> String {
        let left = max(r.duration - r.position, 0)
        let m = Int(left) / 60
        return m >= 60 ? "\(m / 60) h \(m % 60) min" : "\(m) min"
    }

    var body: some View {
        HStack(spacing: 12) {
            ZStack {
                if isHovering {
                    Button(action: onPlay) {
                        Image(systemName: "play.fill")
                            .font(.system(size: 12, weight: .bold))
                            .foregroundStyle(.primary)
                    }
                    .buttonStyle(.plain)
                } else if isWatched {
                    Image(systemName: "checkmark")
                        .font(.system(size: 11, weight: .bold))
                        .foregroundStyle(Theme.accent)
                } else {
                    Text("\(episode.episodeNumber ?? fallbackNumber)")
                        .font(.system(size: 12.5))
                        .monospacedDigit()
                        .foregroundStyle(.secondary)
                }
            }
            .frame(width: 32, alignment: .center)

            VStack(alignment: .leading, spacing: 2) {
                Text(episode.episodeRowTitle)
                    .font(.system(size: 13, weight: .medium))
                    .foregroundStyle(isWatched ? .secondary : .primary)
                    .lineLimit(1)
                if resume != nil, let r = watch.record(for: episode.id) {
                    HStack(spacing: 6) {
                        GeometryReader { geo in
                            ZStack(alignment: .leading) {
                                Capsule().fill(Color.white.opacity(0.14))
                                Capsule().fill(Theme.accent)
                                    .frame(width: max(3, geo.size.width * r.progress))
                            }
                        }
                        .frame(width: 90, height: 3)
                        Text("reste \(remaining(r))")
                            .font(.system(size: 10.5))
                            .foregroundStyle(Theme.accent)
                        Text(verbatim: "· \(r.formattedPosition)")
                            .font(.system(size: 10.5))
                            .foregroundStyle(.tertiary)
                    }
                    .frame(height: 11)
                    .help("Reprendre à \(r.formattedPosition)")
                } else {
                    Text(episode.filename)
                        .font(.system(size: 11))
                        .foregroundStyle(.tertiary)
                        .lineLimit(1)
                }
            }

            Spacer(minLength: 8)

            Text(episode.formattedSize)
                .font(.system(size: 11.5))
                .foregroundStyle(.secondary)
                .monospacedDigit()
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 7)
        .background(
            RoundedRectangle(cornerRadius: 7, style: .continuous)
                .fill(isSelected ? Color.white.opacity(0.10)
                                 : (isHovering ? Color.white.opacity(0.05) : .clear)))
        .contentShape(Rectangle())
        .onHover { isHovering = $0 }
        .onTapGesture(count: 2, perform: onPlay)
        .onTapGesture(perform: onSelect)
    }
}
