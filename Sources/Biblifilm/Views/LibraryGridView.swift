import SwiftUI

/// The main content area: poster grid, or a guidance state when there's nothing to show.
struct LibraryGridView: View {
    @ObservedObject var model: LibraryModel
    @ObservedObject private var settings = AppSettings.shared
    @State private var bannerDismissed = false

    private let columns = [GridItem(.adaptive(minimum: 148, maximum: 210), spacing: 16)]

    var body: some View {
        Group {
            if model.libraryPath == nil {
                EmptyStateView(
                    symbol: "folder.badge.questionmark",
                    title: "Aucun dossier sélectionné",
                    message: "Choisissez le dossier qui contient vos films et séries. Biblifilm l'analysera et retrouvera les titres et les jaquettes.",
                    actionTitle: "Choisir un dossier…") { model.chooseFolder() }
            } else if model.isScanning && model.items.isEmpty {
                VStack(spacing: 14) {
                    ProgressView().controlSize(.large)
                    Text(model.scanProgress.isEmpty ? "Analyse…" : model.scanProgress)
                        .foregroundStyle(.secondary)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else if let season = model.openedSeason {
                SeasonDetailView(model: model, group: season)
            } else if model.visibleEntries.isEmpty {
                EmptyStateView(
                    symbol: model.searchText.isEmpty ? "film.stack" : "magnifyingglass",
                    title: model.searchText.isEmpty ? "Rien à afficher ici" : "Aucun résultat",
                    message: model.searchText.isEmpty
                        ? "Aucun média ne correspond à cette catégorie."
                        : "Aucun média ne correspond à « \(model.searchText) ».",
                    actionTitle: nil, action: nil)
            } else {
                grid
            }
        }
        .background(Theme.contentBackground)
        .safeAreaInset(edge: .top, spacing: 0) { keyBanner }
    }

    /// Films can't be identified at all without a TMDB key, so say so plainly
    /// rather than leaving a wall of "non identifiés".
    @ViewBuilder
    private var keyBanner: some View {
        let unmatched = model.counts[.unmatched] ?? 0
        if !settings.hasTMDBKey, unmatched > 0, !bannerDismissed,
           model.libraryPath != nil, model.openedSeason == nil {
            HStack(spacing: 11) {
                Image(systemName: "key.horizontal.fill")
                    .foregroundStyle(Theme.accent)
                VStack(alignment: .leading, spacing: 1) {
                    Text("\(unmatched) fichiers non identifiés")
                        .font(.system(size: 12.5, weight: .semibold))
                    Text("Sans clé TMDB, les films ne peuvent pas être identifiés — seules les séries le sont.")
                        .font(.system(size: 11.5))
                        .foregroundStyle(.secondary)
                }
                Spacer(minLength: 8)
                SettingsLink {
                    Text("Ajouter une clé…")
                }
                .buttonStyle(.borderedProminent)
                .tint(Theme.accent)
                .controlSize(.small)
                Button {
                    bannerDismissed = true
                } label: {
                    Image(systemName: "xmark")
                        .font(.system(size: 10, weight: .bold))
                        .foregroundStyle(.secondary)
                }
                .buttonStyle(.plain)
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 9)
            .background(.ultraThinMaterial)
            .overlay(alignment: .bottom) { Divider().opacity(0.6) }
        }
    }

    private var grid: some View {
        ScrollView {
            LazyVGrid(columns: columns, alignment: .leading, spacing: 18) {
                ForEach(model.visibleEntries) { entry in
                    EntryCard(
                        entry: entry,
                        isSelected: isSelected(entry),
                        onPrimary: { activate(entry) },
                        onPlay: { model.play(entry) })
                    .onTapGesture(count: 2) { activate(entry) }
                    .onTapGesture { select(entry) }
                    .contextMenu { contextMenu(for: entry) }
                }
            }
            .padding(.horizontal, 22)
            .padding(.vertical, 18)
        }
        // A click that misses a poster clears the selection, which closes
        // the info panel. Card taps are handled first and never reach here.
        .contentShape(Rectangle())
        .onTapGesture { model.clearSelection() }
    }

    private func isSelected(_ entry: LibraryEntry) -> Bool {
        if case .single(let item) = entry { return model.selectedItemID == item.id }
        return false
    }

    /// A season opens its episode list; a film just plays.
    private func activate(_ entry: LibraryEntry) {
        switch entry {
        case .single(let item):  model.play(item)
        case .season(let group): model.open(group)
        }
    }

    private func select(_ entry: LibraryEntry) {
        switch entry {
        case .single(let item):  model.select(item.id)
        case .season(let group): model.open(group)
        }
    }

    @ViewBuilder
    private func contextMenu(for entry: LibraryEntry) -> some View {
        switch entry {
        case .single(let item):
            contextMenu(for: item)
        case .season(let group):
            Button("Voir les épisodes") { model.open(group) }
            Button("Lire la saison dans VLC") { model.play(entry) }
            Divider()
            Button("Marquer la saison comme vue") {
                WatchStore.shared.setWatched(true, for: group.episodes)
            }
            Button("Marquer comme non vue") {
                WatchStore.shared.setWatched(false, for: group.episodes)
            }
            Divider()
            Button("Rechercher les infos à nouveau") {
                for episode in group.episodes { model.refetch(episode) }
            }
        }
    }

    @ViewBuilder
    private func contextMenu(for item: MediaItem) -> some View {
        Button("Lire dans VLC") { model.play(item) }
        if WatchStore.shared.resumePosition(for: item) != nil {
            Button("Lire depuis le début") { model.play(item, fromStart: true) }
        }
        Button(WatchStore.shared.isWatched(item) ? "Marquer comme non vu" : "Marquer comme vu") {
            WatchStore.shared.setWatched(!WatchStore.shared.isWatched(item), for: item)
        }
        Divider()
        Button("Ouvrir dans VLC") { model.openExternally(item) }
        Button("Afficher dans le Finder") { model.reveal(item) }
        Divider()
        Button("Rechercher les infos à nouveau") { model.refetch(item) }
        Button("Copier le chemin") {
            NSPasteboard.general.clearContents()
            NSPasteboard.general.setString(item.url.path, forType: .string)
        }
    }
}

struct EmptyStateView: View {
    let symbol: String
    let title: String
    let message: String
    var actionTitle: String?
    var action: (() -> Void)?

    var body: some View {
        VStack(spacing: 13) {
            Image(systemName: symbol)
                .font(.system(size: 46, weight: .thin))
                .foregroundStyle(.tertiary)
            Text(title).font(.title3.weight(.semibold))
            Text(message)
                .font(.subheadline)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
                .frame(maxWidth: 380)
            if let actionTitle, let action {
                Button(actionTitle, action: action)
                    .buttonStyle(.borderedProminent)
                    .tint(Theme.accent)
                    .controlSize(.large)
                    .padding(.top, 4)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}
