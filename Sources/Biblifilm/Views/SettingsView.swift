import SwiftUI

struct SettingsView: View {
    @ObservedObject var settings = AppSettings.shared
    @ObservedObject var model: LibraryModel

    @State private var cacheInfo = ""
    @State private var restoreMessage: String?
    @State private var keyStatus: KeyStatus = .unknown
    @State private var isTestingKey = false

    private enum KeyStatus {
        case unknown, valid, invalid(String)
    }

    var body: some View {
        TabView {
            general.tabItem { Label("Général", systemImage: "gearshape") }
            metadata.tabItem { Label("Métadonnées", systemImage: "sparkle.magnifyingglass") }
        }
        .frame(width: 480, height: 355)
        .task { await refreshCacheInfo() }
    }

    private var general: some View {
        Form {
            Section {
                LabeledContent("Dossier") {
                    HStack {
                        Text(settings.libraryPath ?? "Aucun")
                            .lineLimit(1).truncationMode(.middle)
                            .foregroundStyle(settings.libraryPath == nil ? .secondary : .primary)
                        Spacer()
                        Button("Choisir…") { model.chooseFolder() }
                    }
                }
                Toggle("Inclure les fichiers audio", isOn: $settings.includeAudio)
                Toggle("Ignorer les fichiers de moins de 20 Mo", isOn: $settings.skipSmallFiles)
                Text("Évite d'importer les extraits, bandes-annonces et échantillons.")
                    .font(.caption).foregroundStyle(.secondary)
                Toggle("Détecter automatiquement les nouveaux fichiers",
                       isOn: $settings.watchFolder)
                    .onChange(of: settings.watchFolder) { model.updateWatching() }
                Text("Surveille le dossier en continu : un film ajouté apparaît tout seul, avec sa jaquette.")
                    .font(.caption).foregroundStyle(.secondary)
            }
            Section {
                Button("Analyser à nouveau") { model.rescan() }
                    .disabled(settings.libraryPath == nil)
            }
            Section {
                LabeledContent("VLC") {
                    Label(
                        VLCLauncher.isVLCInstalled ? "Installé" : "Introuvable",
                        systemImage: VLCLauncher.isVLCInstalled
                            ? "checkmark.circle.fill" : "exclamationmark.triangle.fill")
                    .foregroundStyle(VLCLauncher.isVLCInstalled ? .green : .orange)
                }
                if !VLCLauncher.isVLCInstalled {
                    Text("VLC n'a pas été trouvé. Les fichiers s'ouvriront avec l'app par défaut.")
                        .font(.caption).foregroundStyle(.secondary)
                }
            }
        }
        .formStyle(.grouped)
    }

    private var metadata: some View {
        Form {
            Section("Clé API TMDB") {
                SecureField("Collez votre clé ici", text: $settings.tmdbAPIKey)
                Text("Gratuite sur themoviedb.org → Paramètres → API. Sans clé, Biblifilm utilise TVmaze et Wikidata, avec des résultats plus limités.")
                    .font(.caption).foregroundStyle(.secondary)
                Link("Obtenir une clé TMDB",
                     destination: URL(string: "https://www.themoviedb.org/settings/api")!)
                    .font(.caption)

                HStack(spacing: 8) {
                    Button("Tester la clé") { testKey() }
                        .disabled(!settings.hasTMDBKey || isTestingKey)
                    if isTestingKey {
                        ProgressView().controlSize(.small)
                    } else {
                        switch keyStatus {
                        case .unknown:
                            EmptyView()
                        case .valid:
                            Label("Clé valide", systemImage: "checkmark.circle.fill")
                                .foregroundStyle(.green).font(.caption)
                        case .invalid(let reason):
                            Label(reason, systemImage: "xmark.circle.fill")
                                .foregroundStyle(.orange).font(.caption)
                                .fixedSize(horizontal: false, vertical: true)
                        }
                    }
                }
                .onChange(of: settings.tmdbAPIKey) { keyStatus = .unknown }
            }

            Section {
                Picker("Langue", selection: $settings.preferredLanguage) {
                    ForEach(AppSettings.availableLanguages, id: \.code) { lang in
                        Text(lang.label).tag(lang.code)
                    }
                }
                Toggle("Rechercher automatiquement après l'analyse", isOn: $settings.autoFetchMetadata)
            }

            Section("Historique de visionnage") {
                LabeledContent("Épisodes suivis") {
                    Text("\(WatchStore.shared.records.count)")
                        .foregroundStyle(.secondary).monospacedDigit()
                }
                Button("Restaurer la sauvegarde précédente") {
                    let n = WatchStore.shared.restoreFromBackup()
                    restoreMessage = n > 0
                        ? "\(n) entrées restaurées."
                        : "Aucune sauvegarde utilisable."
                }
                if let restoreMessage {
                    Text(restoreMessage).font(.caption).foregroundStyle(.secondary)
                }
            }

            Section {
                Button("Tout rechercher à nouveau") { model.fetchMetadata(force: true) }
                    .disabled(model.items.isEmpty)
                Button("Vider le cache") {
                    Task {
                        await MetadataService.shared.clearCache()
                        await ArtworkCache.shared.clear()
                        await refreshCacheInfo()
                    }
                }
                if !cacheInfo.isEmpty {
                    Text(cacheInfo).font(.caption).foregroundStyle(.secondary)
                }
            }
        }
        .formStyle(.grouped)
    }

    private func testKey() {
        isTestingKey = true
        Task {
            let problem = await TMDBProvider.validate(key: settings.tmdbAPIKey)
            isTestingKey = false
            keyStatus = problem.map { KeyStatus.invalid($0) } ?? .valid
            // A newly working key is worth re-running lookups that previously missed.
            if problem == nil { model.fetchMetadata(force: true) }
        }
    }

    private func refreshCacheInfo() async {
        let count = await MetadataService.shared.cacheCount
        let bytes = await ArtworkCache.shared.diskSize()
        let size = ByteCountFormatter.string(fromByteCount: bytes, countStyle: .file)
        cacheInfo = "\(count) fiches en cache · \(size) de jaquettes"
    }
}
