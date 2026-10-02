import SwiftUI

/// Music-style source list: uppercase section headers, symbol + label rows.
struct SidebarView: View {
    @ObservedObject var model: LibraryModel
    @AppStorage("seriesSectionExpanded") private var seriesExpanded = true

    var body: some View {
        List(selection: $model.selection) {
            Section("Bibliothèque") {
                row(.all)
                if (model.counts[.continueWatching] ?? 0) > 0 { row(.continueWatching) }
                row(.movies)
                row(.shows)
                if AppSettings.shared.includeAudio { row(.audio) }
                row(.recent)
                if (model.counts[.unmatched] ?? 0) > 0 { row(.unmatched) }
            }

            if !model.seriesNames.isEmpty {
                Section(isExpanded: $seriesExpanded) {
                    ForEach(model.seriesNames, id: \.self) { name in
                        Label(name, systemImage: "rectangle.stack")
                            .lineLimit(1)
                            .tag(LibrarySection.series(name))
                    }
                } header: {
                    // The whole title folds the list, not just the chevron
                    // macOS shows on hover.
                    Text("Séries")
                        .contentShape(Rectangle())
                        .onTapGesture {
                            withAnimation { seriesExpanded.toggle() }
                        }
                }
            }
        }
        .listStyle(.sidebar)
        .safeAreaInset(edge: .bottom) { footer }
    }

    @ViewBuilder
    private func row(_ section: LibrarySection) -> some View {
        HStack {
            Label(section.title, systemImage: section.symbol)
            Spacer()
            if let count = model.counts[section], count > 0 {
                Text("\(count)")
                    .font(.caption)
                    .foregroundStyle(.tertiary)
                    .monospacedDigit()
            }
        }
        .tag(section)
    }

    private var footer: some View {
        VStack(spacing: 6) {
            Divider().opacity(0.5)
            Button {
                model.chooseFolder()
            } label: {
                HStack(spacing: 6) {
                    Image(systemName: "folder.badge.plus")
                    Text(model.libraryPath == nil ? "Choisir un dossier…" : "Changer de dossier…")
                        .lineLimit(1)
                    Spacer()
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .font(.subheadline)
            .foregroundStyle(.secondary)
            .padding(.horizontal, 12)
            .padding(.bottom, 10)
        }
    }
}
