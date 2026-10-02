import SwiftUI

/// Right-hand inspector showing everything known about the selected item.
struct DetailView: View {
    @ObservedObject var model: LibraryModel
    let item: MediaItem

    @ObservedObject private var watch = WatchStore.shared
    @State private var isCorrecting = false

    private var meta: MediaMetadata? {
        guard let m = item.metadata, m.matched else { return nil }
        return m
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                artwork

                VStack(alignment: .leading, spacing: 4) {
                    Text(item.displayTitle)
                        .font(.system(size: 17, weight: .semibold))
                        .fixedSize(horizontal: false, vertical: true)
                    if let series = meta?.seriesTitle, item.kind == .episode {
                        Text(series)
                            .font(.subheadline)
                            .foregroundStyle(Theme.accent)
                    }
                    Text(item.displaySubtitle)
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                }

                playButton

                if let overview = meta?.overview, !overview.isEmpty {
                    VStack(alignment: .leading, spacing: 5) {
                        Text("Synopsis").font(.caption.weight(.semibold)).foregroundStyle(.secondary)
                        Text(overview)
                            .font(.callout)
                            .foregroundStyle(.primary.opacity(0.9))
                            .fixedSize(horizontal: false, vertical: true)
                    }
                }

                if let genres = meta?.genres, !genres.isEmpty {
                    FlowTags(tags: genres)
                }

                Divider().opacity(0.4)
                fileInfo

                if item.metadata?.matched != true { unmatchedNotice }
            }
            .padding(18)
        }
        .background(Theme.windowBackground)
        .sheet(isPresented: $isCorrecting) {
            // Seeded with what the *filename* says, not the match on screen:
            // the reason for opening this sheet is usually that the match is
            // wrong, and "Layer Cake" is a useless place to search from.
            CorrectionSheet(model: model, item: item, initialTitle: item.parsed.title) {
                isCorrecting = false
            }
        }
    }

    private var artwork: some View {
        ArtworkView(
            urlString: meta?.stillURL ?? meta?.posterURL,
            seed: item.groupingTitle,
            symbol: item.kind.symbol,
            aspect: meta?.stillURL != nil ? Theme.stillAspect
                  : (item.kind == .audio ? 1 : Theme.posterAspect),
            cornerRadius: 10)
        .shadow(color: .black.opacity(0.45), radius: 12, y: 5)
        .frame(maxWidth: .infinity)
    }

    private var playButton: some View {
        VStack(spacing: 8) {
            Button {
                model.play(item)
            } label: {
                Label(watch.resumePosition(for: item) != nil
                        ? "Reprendre à \(watch.record(for: item.id)?.formattedPosition ?? "")"
                        : "Lire dans VLC",
                      systemImage: "play.fill")
                    .frame(maxWidth: .infinity)
            }
            .buttonStyle(.borderedProminent)
            .tint(Theme.accent)
            .controlSize(.large)

            if watch.resumePosition(for: item) != nil {
                Button {
                    model.play(item, fromStart: true)
                } label: {
                    Label("Reprendre au début", systemImage: "gobackward")
                        .frame(maxWidth: .infinity)
                }
                .buttonStyle(.bordered)
            }

            Button {
                model.openExternally(item)
            } label: {
                Label("Ouvrir dans VLC", systemImage: "arrow.up.forward.app")
                    .frame(maxWidth: .infinity)
            }
            .buttonStyle(.bordered)

            Button {
                watch.setWatched(!watch.isWatched(item), for: item)
            } label: {
                Label(watch.isWatched(item) ? "Marquer comme non vu" : "Marquer comme vu",
                      systemImage: watch.isWatched(item) ? "eye.slash" : "checkmark.circle")
                    .frame(maxWidth: .infinity)
            }
            .buttonStyle(.bordered)

            HStack(spacing: 8) {
                Button {
                    model.reveal(item)
                } label: {
                    Label("Finder", systemImage: "folder")
                        .frame(maxWidth: .infinity)
                }
                Button {
                    isCorrecting = true
                } label: {
                    Label("Corriger", systemImage: "wand.and.stars")
                        .frame(maxWidth: .infinity)
                }
            }
            .buttonStyle(.bordered)
            .controlSize(.regular)
        }
    }

    private var fileInfo: some View {
        VStack(alignment: .leading, spacing: 7) {
            Text("Fichier").font(.caption.weight(.semibold)).foregroundStyle(.secondary)
            infoRow("Nom", item.filename)
            infoRow("Type", item.kind.displayName)
            infoRow("Taille", item.formattedSize)
            infoRow("Format", item.url.pathExtension.uppercased())
            if let r = meta?.rating, r > 0 {
                infoRow("Note", String(format: "%.1f/10", r))
            }
            if let source = meta?.source { infoRow("Source", source) }
            if let group = item.parsed.releaseGroup { infoRow("Groupe", group) }
        }
    }

    private func infoRow(_ label: String, _ value: String) -> some View {
        HStack(alignment: .top, spacing: 8) {
            Text(label)
                .font(.caption)
                .foregroundStyle(.secondary)
                .frame(width: 54, alignment: .leading)
            Text(value)
                .font(.caption)
                .textSelection(.enabled)
                .fixedSize(horizontal: false, vertical: true)
            Spacer(minLength: 0)
        }
    }

    private var unmatchedNotice: some View {
        HStack(alignment: .top, spacing: 8) {
            Image(systemName: "info.circle").foregroundStyle(.secondary)
            VStack(alignment: .leading, spacing: 3) {
                Text("Titre non identifié").font(.caption.weight(.semibold))
                Text(AppSettings.shared.hasTMDBKey
                     ? "Aucune correspondance trouvée. Essayez « Corriger » pour choisir le bon titre."
                     : "Ajoutez une clé TMDB dans les réglages pour de bien meilleurs résultats.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .padding(10)
        .background(RoundedRectangle(cornerRadius: 8).fill(Color.white.opacity(0.05)))
    }
}

/// Pick the right title yourself.
///
/// The automatic ranking cannot always be trusted: "Pusher 2" matches neither
/// *Pusher II* nor *Layer Cake* on title -- the catalogue writes the sequel in
/// Roman numerals -- so the two are separated by popularity alone, and the wrong
/// one wins. Showing the alternatives and letting the user point at one settles
/// it without guessing.
struct CorrectionSheet: View {
    @ObservedObject var model: LibraryModel
    let item: MediaItem
    let onDone: () -> Void

    @State private var query: String
    @State private var results: [MatchCandidate] = []
    @State private var isSearching = false
    @State private var hasSearched = false

    init(model: LibraryModel, item: MediaItem, initialTitle: String,
         onDone: @escaping () -> Void) {
        self.model = model
        self.item = item
        self.onDone = onDone
        _query = State(initialValue: initialTitle)
    }

    private var isSeries: Bool { item.kind == .episode }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Corriger le titre").font(.headline)
            Text(isSeries
                 ? "Choisissez la bonne série. La correction s'applique à tous les épisodes."
                 : "Choisissez le bon film dans la liste.")
                .font(.caption)
                .foregroundStyle(.secondary)

            HStack(spacing: 8) {
                TextField("Titre", text: $query)
                    .textFieldStyle(.roundedBorder)
                    .onSubmit { Task { await search() } }
                Button("Rechercher") { Task { await search() } }
                    .keyboardShortcut(.defaultAction)
                    .disabled(isSearching || query.trimmingCharacters(in: .whitespaces).isEmpty)
            }

            resultList

            HStack {
                Button("Recherche automatique") {
                    model.refetch(item, overrideTitle: query)
                    onDone()
                }
                .help("Relance la recherche et garde le meilleur résultat.")
                Spacer()
                Button("Annuler") { onDone() }
                    .keyboardShortcut(.cancelAction)
            }
        }
        .padding(20)
        .frame(width: 470)
        .task { await search() }
    }

    @ViewBuilder
    private var resultList: some View {
        if !AppSettings.shared.hasTMDBKey {
            notice("Ajoutez une clé TMDB dans les réglages pour choisir parmi les titres.")
        } else if isSearching {
            HStack(spacing: 8) {
                ProgressView().controlSize(.small)
                Text("Recherche…").font(.caption).foregroundStyle(.secondary)
            }
            .frame(maxWidth: .infinity, minHeight: 90)
        } else if results.isEmpty {
            notice(hasSearched
                   ? "Aucun résultat. Essayez le titre original, sans l'année."
                   : "Saisissez un titre puis lancez la recherche.")
        } else {
            ScrollView {
                VStack(spacing: 2) {
                    ForEach(results) { candidate in
                        row(candidate)
                        if candidate.id != results.last?.id { Divider().opacity(0.25) }
                    }
                }
            }
            .frame(height: 300)
            .background(RoundedRectangle(cornerRadius: 8).fill(Color.white.opacity(0.05)))
        }
    }

    private func row(_ candidate: MatchCandidate) -> some View {
        Button {
            model.apply(candidate, to: item, overrideTitle: query)
            onDone()
        } label: {
            HStack(alignment: .top, spacing: 10) {
                ArtworkView(urlString: candidate.posterURL,
                            seed: candidate.title,
                            symbol: candidate.isSeries ? "tv" : "film",
                            cornerRadius: 4)
                    .frame(width: 44)
                VStack(alignment: .leading, spacing: 2) {
                    Text(candidate.title)
                        .font(.callout.weight(.medium))
                        .lineLimit(1)
                    if !candidate.subtitle.isEmpty {
                        Text(candidate.subtitle)
                            .font(.caption)
                            .foregroundStyle(Theme.accent)
                            .lineLimit(1)
                    }
                    if let overview = candidate.overview, !overview.isEmpty {
                        Text(overview)
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                            .lineLimit(2)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                }
                Spacer(minLength: 0)
            }
            .padding(8)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }

    private func notice(_ text: String) -> some View {
        Text(text)
            .font(.caption)
            .foregroundStyle(.secondary)
            .frame(maxWidth: .infinity, minHeight: 90)
            .multilineTextAlignment(.center)
    }

    private func search() async {
        let typed = query.trimmingCharacters(in: .whitespaces)
        guard !typed.isEmpty, !isSearching else { return }
        isSearching = true
        results = await model.candidates(for: item, overrideTitle: typed)
        isSearching = false
        hasSearched = true
    }
}

/// Simple wrapping row of genre chips.
struct FlowTags: View {
    let tags: [String]

    var body: some View {
        HStack(spacing: 6) {
            ForEach(tags.prefix(3), id: \.self) { tag in
                Text(tag)
                    .font(.caption2)
                    .padding(.horizontal, 8)
                    .padding(.vertical, 3)
                    .background(Capsule().fill(Color.white.opacity(0.10)))
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }
        }
    }
}
