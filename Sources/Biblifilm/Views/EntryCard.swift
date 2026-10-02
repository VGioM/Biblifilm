import SwiftUI

/// One grid tile: a film, or a season shown as a stack of episodes.
struct EntryCard: View {
    let entry: LibraryEntry
    let isSelected: Bool
    let onPrimary: () -> Void      // open a season / select a film
    let onPlay: () -> Void

    @ObservedObject private var watch = WatchStore.shared
    @State private var isHovering = false

    /// How far through this tile the viewer is, and whether it is finished.
    private var progress: Double {
        switch entry {
        case .single(let item):  return watch.isWatched(item) ? 1 : watch.progress(for: item)
        case .season(let group): return watch.progress(in: group)
        }
    }

    private var isFullyWatched: Bool {
        switch entry {
        case .single(let item):  return watch.isWatched(item)
        case .season(let group): return watch.watchedCount(in: group) == group.episodes.count
                                     && !group.episodes.isEmpty
        }
    }

    /// "3/14 vus" for a part-watched season.
    private var watchedLabel: String? {
        guard case .season(let group) = entry else { return nil }
        let seen = watch.watchedCount(in: group)
        guard seen > 0 else { return nil }
        return "\(seen)/\(group.episodes.count) vus"
    }

    private var isSeason: Bool {
        if case .season = entry { return true }
        return false
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 7) {
            ZStack(alignment: .top) {
                if isSeason {
                    // Layers peeking out behind the cover read as "several episodes".
                    RoundedRectangle(cornerRadius: Theme.posterRadius, style: .continuous)
                        .fill(Color.white.opacity(0.13))
                        .padding(.horizontal, 15)
                        .offset(y: -9)
                    RoundedRectangle(cornerRadius: Theme.posterRadius, style: .continuous)
                        .fill(Color.white.opacity(0.20))
                        .padding(.horizontal, 7.5)
                        .offset(y: -4.5)
                }

                artwork
            }
            .padding(.top, isSeason ? 10 : 0)

            VStack(alignment: .leading, spacing: 2) {
                Text(entry.displayTitle)
                    .font(.system(size: 12.5, weight: .medium))
                    .lineLimit(1)
                HStack(spacing: 5) {
                    Text(entry.displaySubtitle)
                        .font(.system(size: 11.5))
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                    if let watchedLabel {
                        Text("· \(watchedLabel)")
                            .font(.system(size: 11.5))
                            .foregroundStyle(Theme.accent.opacity(0.9))
                            .lineLimit(1)
                    }
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .padding(7)
        .background(
            RoundedRectangle(cornerRadius: 10, style: .continuous)
                .fill(isSelected ? Color.white.opacity(0.10) : .clear))
        .animation(.easeOut(duration: 0.16), value: isHovering)
        .onHover { isHovering = $0 }
        .contentShape(Rectangle())
    }

    private var artwork: some View {
        ZStack {
            ArtworkView(
                urlString: entry.posterURL,
                seed: entry.sortTitle,
                symbol: entry.kind.symbol,
                aspect: entry.kind == .audio ? 1 : Theme.posterAspect)

            if isHovering {
                RoundedRectangle(cornerRadius: Theme.posterRadius, style: .continuous)
                    .fill(.black.opacity(0.34))
                    .transition(.opacity)

                HStack(spacing: 10) {
                    circleButton(isSeason ? "list.bullet" : "play.fill", action: onPrimary)
                    if isSeason {
                        circleButton("play.fill", action: onPlay)
                    }
                }
                .transition(.scale(scale: 0.85).combined(with: .opacity))
            }

            // Netflix-style resume bar along the bottom of the cover.
            if progress > 0.01 && !isFullyWatched {
                VStack {
                    Spacer()
                    GeometryReader { geo in
                        ZStack(alignment: .leading) {
                            Rectangle().fill(.black.opacity(0.55))
                            Rectangle().fill(Theme.accent)
                                .frame(width: max(3, geo.size.width * progress))
                        }
                    }
                    .frame(height: 3.5)
                }
                .clipShape(RoundedRectangle(cornerRadius: Theme.posterRadius,
                                            style: .continuous))
            }

            if isFullyWatched {
                Image(systemName: "checkmark.circle.fill")
                    .font(.system(size: 15))
                    .foregroundStyle(.white, Theme.accent)
                    .padding(7)
                    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topTrailing)
            }

            if isSeason {
                // Episode count, bottom-right like a badge on a stack.
                Text("\(entry.episodeCount)")
                    .font(.system(size: 11, weight: .semibold))
                    .monospacedDigit()
                    .foregroundStyle(.white)
                    .padding(.horizontal, 7)
                    .padding(.vertical, 3)
                    .background(Capsule().fill(.black.opacity(0.68)))
                    .overlay(Capsule().strokeBorder(.white.opacity(0.28), lineWidth: 0.5))
                    .padding(7)
                    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .bottomTrailing)
            }
        }
        .shadow(color: .black.opacity(0.42), radius: isHovering ? 11 : 5,
                x: 0, y: isHovering ? 6 : 3)
        .scaleEffect(isHovering ? 1.022 : 1)
    }

    private func circleButton(_ symbol: String, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Image(systemName: symbol)
                .font(.system(size: 15, weight: .bold))
                .foregroundStyle(.white)
                .frame(width: 40, height: 40)
                .background(Circle().fill(.black.opacity(0.55)))
                .overlay(Circle().strokeBorder(.white.opacity(0.85), lineWidth: 1.2))
        }
        .buttonStyle(.plain)
    }
}
