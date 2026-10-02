import SwiftUI

/// Music.app's dark-mode greys with the system blue for the accent.
/// Shared with Biblijeux so the two apps read as one family: a colour
/// changed here has to be changed there too.
enum Theme {
    static let accent = Color(red: 0.04, green: 0.52, blue: 1.00)      // macOS system blue
    static let windowBackground = Color(red: 0.13, green: 0.13, blue: 0.14)
    static let contentBackground = Color(red: 0.11, green: 0.11, blue: 0.12)
    static let cardBackground = Color(red: 0.17, green: 0.17, blue: 0.18)
    static let hairline = Color.white.opacity(0.08)

    static let posterRadius: CGFloat = 8
    static let posterAspect: CGFloat = 2.0 / 3.0    // standard movie poster
    static let stillAspect: CGFloat = 16.0 / 9.0

    /// Deterministic placeholder gradient so an artwork-less card still looks intentional.
    static func placeholderGradient(for seed: String) -> LinearGradient {
        var hash = 5381
        for byte in seed.utf8 { hash = (hash &* 33 &+ Int(byte)) & 0xFFFFFF }
        let hue = Double(hash % 360) / 360.0
        return LinearGradient(
            colors: [
                Color(hue: hue, saturation: 0.38, brightness: 0.42),
                Color(hue: (hue + 0.08).truncatingRemainder(dividingBy: 1),
                      saturation: 0.45, brightness: 0.24)
            ],
            startPoint: .topLeading, endPoint: .bottomTrailing)
    }
}

/// Loads artwork through the on-disk cache, with a graceful placeholder.
struct ArtworkView: View {
    let urlString: String?
    let seed: String
    var symbol: String = "film"
    var aspect: CGFloat = Theme.posterAspect
    var cornerRadius: CGFloat = Theme.posterRadius

    @State private var image: NSImage?
    @State private var loadedURL: String?
    @State private var isLoading = false

    var body: some View {
        ZStack {
            if let image {
                Image(nsImage: image)
                    .resizable()
                    .aspectRatio(contentMode: .fill)
            } else {
                Theme.placeholderGradient(for: seed)
                Image(systemName: symbol)
                    .font(.system(size: 26, weight: .light))
                    .foregroundStyle(.white.opacity(0.45))
                if isLoading {
                    ProgressView()
                        .controlSize(.small)
                        .opacity(0.6)
                }
            }
        }
        .aspectRatio(aspect, contentMode: .fit)
        .clipShape(RoundedRectangle(cornerRadius: cornerRadius, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)
                .strokeBorder(Color.white.opacity(0.09), lineWidth: 0.5))
        .task(id: urlString) { await load() }
    }

    private func load() async {
        guard let urlString, !urlString.isEmpty else {
            image = nil
            loadedURL = nil
            return
        }
        // Keep showing the current artwork if this is the same image re-appearing
        // (scrolling, window resize) -- clearing it first causes a visible flash.
        guard loadedURL != urlString else { return }

        isLoading = true
        let loaded = await ArtworkCache.shared.image(for: urlString)
        isLoading = false
        guard !Task.isCancelled else { return }
        loadedURL = loaded != nil ? urlString : nil
        withAnimation(.easeOut(duration: 0.22)) { image = loaded }
    }
}
