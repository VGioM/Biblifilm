import SwiftUI

struct RootView: View {
    @ObservedObject var model: LibraryModel
    /// Not observed: the player publishes its clock four times a second, and
    /// this view holds the whole library, toolbar and search field included.
    /// Re-evaluating all of that on every tick of a film nobody is browsing
    /// is wasted work, so only whether the player is showing is followed.
    private let player = PlayerModel.shared
    @State private var playerShowing = PlayerModel.shared.isPresenting

    var body: some View {
        ZStack {
            library
            // Playback takes over the whole window, like Netflix.
            if playerShowing {
                PlayerScreen(player: player, model: model)
                    .transition(.opacity)
                    .zIndex(1)
            }
        }
        .toolbar(playerShowing ? .hidden : .automatic, for: .windowToolbar)
        .toolbarBackground(playerShowing ? .hidden : .automatic, for: .windowToolbar)
        .animation(.easeOut(duration: 0.22), value: playerShowing)
        .onReceive(player.$current.map { $0 != nil }.removeDuplicates()) { showing in
            if showing != playerShowing { playerShowing = showing }
        }
    }

    private var library: some View {
        NavigationSplitView {
            SidebarView(model: model)
                .navigationSplitViewColumnWidth(min: 190, ideal: 215, max: 300)
        } detail: {
            HStack(spacing: 0) {
                LibraryGridView(model: model)
                    .frame(maxWidth: .infinity)

                if model.inspectorVisible, let item = model.selectedItem {
                    Divider()
                    DetailView(model: model, item: item)
                        .frame(width: 305)
                        .transition(.move(edge: .trailing))
                }
            }
            .navigationTitle(model.openedSeason?.seriesTitle ?? model.selection.title)
            .navigationSubtitle(subtitle)
            .toolbar { toolbarContent }
        }
        .animation(.easeOut(duration: 0.2), value: model.inspectorVisible)
        .animation(.easeOut(duration: 0.2), value: model.selectedItemID != nil)
        .searchable(text: $model.searchText, placement: .toolbar, prompt: "Rechercher")
        .frame(minWidth: 940, minHeight: 580)
        .preferredColorScheme(.dark)
        .tint(Theme.accent)
        .alert("Erreur", isPresented: Binding(
            get: { model.errorMessage != nil },
            set: { if !$0 { model.errorMessage = nil } })
        ) {
            Button("OK") { model.errorMessage = nil }
        } message: {
            Text(model.errorMessage ?? "")
        }
        .task { model.rescanIfNeeded() }
    }

    private var subtitle: String {
        if model.isScanning { return model.scanProgress }
        if model.isFetchingMetadata {
            return "Recherche des infos… \(Int(model.metadataProgress * 100)) %"
        }
        if let season = model.openedSeason { return season.subtitle }
        let n = model.visibleEntries.count
        return n == 0 ? "" : "\(n) élément\(n > 1 ? "s" : "")"
    }

    @ToolbarContentBuilder
    private var toolbarContent: some ToolbarContent {
        if model.openedSeason != nil {
            ToolbarItem(placement: .navigation) {
                Button {
                    model.closeSeason()
                } label: {
                    Label("Retour", systemImage: "chevron.left")
                }
                .help("Retour à la bibliothèque")
                .keyboardShortcut("[", modifiers: .command)
            }
        }

        ToolbarItem(placement: .navigation) {
            Button {
                model.playAllVisible()
            } label: {
                Label("Tout lire", systemImage: "play.circle")
            }
            .help("Tout lire dans VLC")
            .disabled(model.visibleEntries.isEmpty)
        }

        ToolbarItem {
            if model.isFetchingMetadata {
                ProgressView(value: model.metadataProgress)
                    .progressViewStyle(.circular)
                    .controlSize(.small)
            }
        }

        ToolbarItem {
            Menu {
                Picker("Trier par", selection: $model.sortOrder) {
                    ForEach(SortOrder.allCases) { Text($0.rawValue).tag($0) }
                }
                Divider()
                Button("Analyser à nouveau") { model.rescan() }
                    .disabled(model.libraryPath == nil)
                Button("Rechercher les infos manquantes") { model.fetchMetadata() }
                    .disabled(model.items.isEmpty)
            } label: {
                Label("Options", systemImage: "ellipsis.circle")
            }
        }

        ToolbarItem {
            Button {
                model.inspectorVisible.toggle()
            } label: {
                Label("Informations", systemImage: "sidebar.right")
            }
            .help("Afficher/masquer les informations")
        }
    }
}
