import SwiftUI

/// Full-window playback, Netflix-style: the video fills the screen and the
/// controls fade away until you move the mouse.
struct PlayerScreen: View {
    @ObservedObject var player: PlayerModel
    @ObservedObject var model: LibraryModel

    @State private var controlsVisible = true
    @State private var idleWatcher: Task<Void, Never>?
    @State private var lastActivity = Date()
    @State private var lastScroll = Date.distantPast
    @State private var lastPointer: NSPoint?
    @State private var scrubbing = false
    @State private var scrubFraction: Double = 0
    @State private var keyMonitor: Any?
    @State private var openMenu: TrackMenu?
    @State private var fullScreen = false
    @State private var hoverFraction: Double?
    @State private var savedChrome: WindowChrome?
    @StateObject private var preview = ScrubPreview()

    private var item: MediaItem? { player.current }

    /// Which track list is showing, if any.
    private enum TrackMenu { case audio, subtitles }

    /// The window the video lives in -- read live, so the Échap handler
    /// never acts on a stale idea of the full-screen state.
    private var playerWindow: NSWindow? { player.drawable?.window ?? NSApp.keyWindow }
    private var isWindowFullScreen: Bool {
        playerWindow?.styleMask.contains(.fullScreen) ?? false
    }

    private func toggleFullScreen() {
        playerWindow?.toggleFullScreen(nil)
    }

    var body: some View {
        ZStack {
            Color.black.ignoresSafeArea()

            VideoSurface(player: player, onActivity: showControls,
                         onScroll: { lastScroll = .now })
                .ignoresSafeArea()

            if player.isBuffering {
                ProgressView().controlSize(.large).tint(.white)
            }

            if let failure = player.failure { errorOverlay(failure) }

            if let hud = player.volumeHUD { volumeHUD(hud) }

            if let info = player.infoHUD { infoHUD(info) }

            if controlsVisible {
                VStack {
                    topBar
                    Spacer()
                    bottomBar
                }
                .transition(.opacity)
            }
        }
        .background(Color.black)
        .animation(.easeOut(duration: 0.2), value: controlsVisible)
        .animation(.easeOut(duration: 0.15), value: player.volumeHUD)
        .animation(.easeOut(duration: 0.15), value: player.infoHUD)
        .onAppear {
            showControls()
            startIdleWatcher()
            installKeyMonitor()
            fullScreen = isWindowFullScreen
            preview.configure(url: item?.url, duration: player.duration)
            // The drawable is attached a runloop later; wait for the window.
            DispatchQueue.main.async { hideTitleBar() }
        }
        .onChange(of: player.current?.id) { _, _ in
            preview.configure(url: player.current?.url, duration: player.duration)
        }
        .onChange(of: player.duration) { _, length in
            preview.configure(url: player.current?.url, duration: length)
        }
        .onReceive(NotificationCenter.default.publisher(
            for: NSWindow.didEnterFullScreenNotification)) { _ in fullScreen = true }
        .onReceive(NotificationCenter.default.publisher(
            for: NSWindow.didExitFullScreenNotification)) { _ in fullScreen = false }
        .onDisappear {
            idleWatcher?.cancel()
            openMenu = nil
            preview.reset()
            NSCursor.setHiddenUntilMouseMoves(false)
            removeKeyMonitor()
            restoreTitleBar()
        }
        .onHover { inside in if inside { showControls() } }
    }

    // MARK: Window chrome

    /// What the window looked like before playback took it over.
    private struct WindowChrome {
        weak var window: NSWindow?
        let titleVisibility: NSWindow.TitleVisibility
        let transparent: Bool
        let fullSizeContent: Bool
    }

    /// Hiding the toolbar still leaves the title bar -- grey over the detail
    /// column and showing the library's title ("Tout – 4 éléments"). During
    /// playback the video runs under it instead, with only the traffic lights
    /// floating on top, as in QuickTime.
    private func hideTitleBar() {
        guard savedChrome == nil, let window = playerWindow else { return }
        savedChrome = WindowChrome(window: window,
                                   titleVisibility: window.titleVisibility,
                                   transparent: window.titlebarAppearsTransparent,
                                   fullSizeContent: window.styleMask.contains(.fullSizeContentView))
        window.titleVisibility = .hidden
        window.titlebarAppearsTransparent = true
        window.styleMask.insert(.fullSizeContentView)
    }

    private func restoreTitleBar() {
        guard let saved = savedChrome, let window = saved.window else { return }
        window.titleVisibility = saved.titleVisibility
        window.titlebarAppearsTransparent = saved.transparent
        if !saved.fullSizeContent { window.styleMask.remove(.fullSizeContentView) }
        savedChrome = nil
    }

    // MARK: Keyboard

    private func installKeyMonitor() {
        removeKeyMonitor()
        keyMonitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { event in
            // Returning nil swallows the event, which also stops the beep.
            handle(event) ? nil : event
        }
    }

    private func removeKeyMonitor() {
        if let keyMonitor { NSEvent.removeMonitor(keyMonitor) }
        keyMonitor = nil
    }

    /// Returns true when the key was consumed by the player.
    private func handle(_ event: NSEvent) -> Bool {
        // Leave menu shortcuts (⌘Q, ⌘W…) alone.
        guard !event.modifierFlags.contains(.command) else { return false }

        switch event.keyCode {
        case 49:                                    // space
            player.togglePlay()
            showControls()
        case 123: player.skip(-10); showControls()  // ←
        case 124: player.skip(10);  showControls()  // →
        case 126: player.nudgeVolume(by: 5)         // ↑
        case 125: player.nudgeVolume(by: -5)        // ↓
        case 11:  player.cycleAudioTrack()          // b, as in VLC
        case 9:   player.cycleSubtitleTrack()       // v, as in VLC
        case 53:                                    // esc
            // Échap leaves full screen first; it only quits the episode when
            // the window is already back to its normal size.
            if openMenu != nil { openMenu = nil; showControls() }
            else if isWindowFullScreen { toggleFullScreen() }
            else { player.close() }
        default:  return false
        }
        return true
    }

    // MARK: Bars

    private var topBar: some View {
        HStack(alignment: .top, spacing: 14) {
            Button {
                player.close()
            } label: {
                Image(systemName: "chevron.left")
                    .font(.system(size: 15, weight: .semibold))
                    .foregroundStyle(.white)
                    .frame(width: 36, height: 36)
                    .background(Circle().fill(.black.opacity(0.45)))
            }
            .buttonStyle(.plain)
            .help("Retour à la bibliothèque (Échap)")

            VStack(alignment: .leading, spacing: 2) {
                Text(item?.playerTitle ?? "")
                    .font(.system(size: 16, weight: .semibold))
                    .foregroundStyle(.white)
                if let sub = item?.playerSubtitle, !sub.isEmpty {
                    Text(sub)
                        .font(.system(size: 12))
                        .foregroundStyle(.white.opacity(0.72))
                }
            }
            Spacer()
        }
        .padding(16)
        .background(
            LinearGradient(colors: [.black.opacity(0.65), .clear],
                           startPoint: .top, endPoint: .bottom)
                .ignoresSafeArea(edges: .top))
    }

    private var bottomBar: some View {
        VStack(spacing: 10) {
            scrubber

            HStack(spacing: 16) {
                controlButton("gobackward.10") { player.skip(-10) }
                controlButton(player.isPlaying ? "pause.fill" : "play.fill", size: 22) {
                    player.togglePlay()
                }
                controlButton("goforward.10") { player.skip(10) }

                if player.canPlayNext {
                    controlButton("forward.end.fill") { player.skipToNext() }
                        .help("Épisode suivant")
                }

                Spacer()

                HStack(spacing: 7) {
                    Image(systemName: player.volume == 0 ? "speaker.slash.fill" : "speaker.wave.2.fill")
                        .font(.system(size: 12))
                        .foregroundStyle(.white.opacity(0.85))
                    Slider(value: Binding(
                        get: { Double(player.volume) },
                        set: { player.volume = Int($0) }), in: 0...150)
                        .frame(width: 96)
                        .tint(.white)
                }

                if player.audioTracks.count > 1 {
                    trackMenuButton(.audio, symbol: "waveform",
                                    help: "Piste audio (B)")
                }
                if !player.subtitleTracks.isEmpty {
                    trackMenuButton(.subtitles,
                                    symbol: player.currentSubtitleTrack >= 0
                                        ? "captions.bubble.fill" : "captions.bubble",
                                    help: "Sous-titres (V)")
                }

                controlButton(fullScreen
                                ? "arrow.down.right.and.arrow.up.left"
                                : "arrow.up.left.and.arrow.down.right", size: 14) {
                    toggleFullScreen()
                }
                .help(fullScreen ? "Quitter le plein écran (Échap)"
                                 : "Plein écran (double-clic)")
            }
        }
        .padding(.horizontal, 18)
        .padding(.bottom, 16)
        .padding(.top, 26)
        .background(
            LinearGradient(colors: [.clear, .black.opacity(0.72)],
                           startPoint: .top, endPoint: .bottom)
                .ignoresSafeArea(edges: .bottom))
    }

    private var scrubber: some View {
        VStack(spacing: 4) {
            GeometryReader { geo in
                let fraction = scrubbing
                    ? scrubFraction
                    : (player.duration > 0 ? player.time / player.duration : 0)
                ZStack(alignment: .leading) {
                    Capsule().fill(.white.opacity(0.22))
                    Capsule().fill(Theme.accent)
                        .frame(width: max(0, geo.size.width * fraction))
                    Circle()
                        .fill(.white)
                        .frame(width: 11, height: 11)
                        .offset(x: max(0, geo.size.width * fraction - 5.5))
                }
                .contentShape(Rectangle())
                .gesture(
                    DragGesture(minimumDistance: 0)
                        .onChanged { value in
                            scrubbing = true
                            scrubFraction = min(max(value.location.x / geo.size.width, 0), 1)
                            preview.request(fraction: scrubFraction)
                            showControls()
                        }
                        .onEnded { _ in
                            player.seek(toFraction: scrubFraction)
                            scrubbing = false
                        })
                // Where the pointer is, which is not where playback is: the
                // vignette answers "what happens if I click here?".
                .onContinuousHover(coordinateSpace: .local) { phase in
                    switch phase {
                    case .active(let point):
                        let position = min(max(point.x / geo.size.width, 0), 1)
                        hoverFraction = position
                        preview.request(fraction: position)
                        showControls()
                    case .ended:
                        hoverFraction = nil
                    }
                }
                .overlay(alignment: .bottomLeading) {
                    // Dragging wins over hovering: the pointer can wander off
                    // the bar mid-scrub and the vignette should follow the drag.
                    if let position = scrubbing ? scrubFraction : hoverFraction,
                       player.duration > 0 {
                        vignette(at: position, barWidth: geo.size.width)
                    }
                }
            }
            .frame(height: 11)

            HStack {
                Text(formatTimecode(scrubbing ? scrubFraction * player.duration : player.time))
                Spacer()
                Text("-" + formatTimecode(max(player.duration - player.time, 0)))
            }
            .font(.system(size: 11).monospacedDigit())
            .foregroundStyle(.white.opacity(0.8))
        }
    }

    /// The scene under the pointer, floating above the seek bar.
    private func vignette(at fraction: Double, barWidth: CGFloat) -> some View {
        let width: CGFloat = 168
        let height = width * 9 / 16
        // Kept inside the bar: at either end the vignette stops instead of
        // hanging off the side of the window.
        let x = min(max(fraction * barWidth - width / 2, 0), max(0, barWidth - width))

        return VStack(spacing: 5) {
            ZStack {
                Color.black
                if let still = preview.image {
                    Image(nsImage: still)
                        .resizable()
                        .aspectRatio(contentMode: .fill)
                        .opacity(preview.isStale ? 0.45 : 1)
                } else {
                    Image(systemName: "film")
                        .font(.system(size: 20, weight: .light))
                        .foregroundStyle(.white.opacity(0.35))
                }
                if preview.isStale || preview.image == nil {
                    ProgressView().controlSize(.small).tint(.white)
                }
            }
            .frame(width: width, height: height)
            .clipShape(RoundedRectangle(cornerRadius: 6, style: .continuous))
            .overlay(RoundedRectangle(cornerRadius: 6, style: .continuous)
                .stroke(.white.opacity(0.28), lineWidth: 1))
            .shadow(color: .black.opacity(0.5), radius: 8, y: 3)

            Text(formatTimecode(fraction * player.duration))
                .font(.system(size: 11, weight: .semibold).monospacedDigit())
                .foregroundStyle(.white)
                .padding(.horizontal, 6)
                .padding(.vertical, 2)
                .background(Capsule().fill(.black.opacity(0.7)))
        }
        .offset(x: x, y: -20)
        .allowsHitTesting(false)
        .transition(.opacity)
    }

    /// A button that drops down the audio or subtitle track list.
    private func trackMenuButton(_ menu: TrackMenu, symbol: String,
                                 help: String) -> some View {
        Button {
            openMenu = (openMenu == menu) ? nil : menu
            showControls()
        } label: {
            Image(systemName: symbol)
                .font(.system(size: 15, weight: .medium))
                .foregroundStyle(.white)
                .frame(width: 34, height: 34)
        }
        .buttonStyle(.plain)
        .help(help)
        .popover(isPresented: Binding(
            get: { openMenu == menu },
            set: { shown in
                if !shown && openMenu == menu { openMenu = nil }
                showControls()
            }), arrowEdge: .top) {
            trackList(menu)
        }
    }

    @ViewBuilder
    private func trackList(_ menu: TrackMenu) -> some View {
        VStack(alignment: .leading, spacing: 1) {
            Text(menu == .audio ? "Piste audio" : "Sous-titres")
                .font(.system(size: 11, weight: .semibold))
                .foregroundStyle(.secondary)
                .padding(.horizontal, 12)
                .padding(.top, 10)
                .padding(.bottom, 4)

            if menu == .audio {
                ForEach(player.audioTracks) { track in
                    TrackRow(title: PlayerModel.displayName(track.name),
                             selected: track.id == player.currentAudioTrack) {
                        player.selectAudioTrack(track.id)
                        openMenu = nil
                        showControls()
                    }
                }
            } else {
                TrackRow(title: "Aucun", selected: player.currentSubtitleTrack < 0) {
                    player.selectSubtitleTrack(-1)
                    openMenu = nil
                    showControls()
                }
                ForEach(player.subtitleTracks) { track in
                    TrackRow(title: PlayerModel.displayName(track.name),
                             selected: track.id == player.currentSubtitleTrack) {
                        player.selectSubtitleTrack(track.id)
                        openMenu = nil
                        showControls()
                    }
                }
            }
        }
        .padding(.bottom, 8)
        .frame(minWidth: 250)
    }

    private func controlButton(_ symbol: String, size: CGFloat = 17,
                               action: @escaping () -> Void) -> some View {
        Button(action: { action(); showControls() }) {
            Image(systemName: symbol)
                .font(.system(size: size, weight: .medium))
                .foregroundStyle(.white)
                .frame(width: 34, height: 34)
        }
        .buttonStyle(.plain)
    }

    // MARK: Overlays

    private func volumeHUD(_ value: Int) -> some View {
        VStack(spacing: 8) {
            Image(systemName: value == 0 ? "speaker.slash.fill" : "speaker.wave.2.fill")
                .font(.system(size: 22))
            Text("\(value) %")
                .font(.system(size: 13, weight: .semibold).monospacedDigit())
            Capsule()
                .fill(.white.opacity(0.25))
                .frame(width: 110, height: 4)
                .overlay(alignment: .leading) {
                    Capsule().fill(.white)
                        .frame(width: 110 * min(Double(value) / 150.0, 1), height: 4)
                }
        }
        .foregroundStyle(.white)
        .padding(20)
        .background(RoundedRectangle(cornerRadius: 14, style: .continuous)
            .fill(.black.opacity(0.62)))
        .transition(.opacity)
        // Scrolling over the readout must keep reaching the video view, which
        // is what turns the volume -- otherwise the gesture stalls under it.
        .allowsHitTesting(false)
    }

    /// Track changes are announced above the control bar, so a change made
    /// with the keyboard is visible without the controls coming back.
    private func infoHUD(_ text: String) -> some View {
        VStack {
            Spacer()
            Text(text)
                .font(.system(size: 13, weight: .medium))
                .foregroundStyle(.white)
                .padding(.horizontal, 16)
                .padding(.vertical, 9)
                .background(Capsule().fill(.black.opacity(0.62)))
                .padding(.bottom, 130)
        }
        .transition(.opacity)
        .allowsHitTesting(false)
    }

    private func errorOverlay(_ message: String) -> some View {
        VStack(spacing: 12) {
            Image(systemName: "exclamationmark.triangle.fill")
                .font(.system(size: 30)).foregroundStyle(.orange)
            Text(message).foregroundStyle(.white).multilineTextAlignment(.center)
            HStack {
                Button("Ouvrir dans VLC") {
                    if let item = player.current {
                        player.close()
                        model.openExternally(item)
                    }
                }
                .buttonStyle(.borderedProminent).tint(Theme.accent)
                Button("Retour") { player.close() }
            }
        }
        .padding(26)
        .background(RoundedRectangle(cornerRadius: 14).fill(.black.opacity(0.8)))
    }

    // MARK: Auto-hide

    private static let idleDelay: TimeInterval = 3

    private func showControls() {
        controlsVisible = true
        lastActivity = .now
        NSCursor.setHiddenUntilMouseMoves(false)
    }

    /// Hides the controls and the pointer once the viewer has been idle for
    /// a while, and keeps the pointer hidden afterwards. A one-shot timer was
    /// not enough: the two-finger volume gesture brings the pointer back
    /// without moving it, so nothing ever hid it again. Re-checking every
    /// half second catches that and anything else that unhides it.
    private func startIdleWatcher() {
        idleWatcher?.cancel()
        idleWatcher = Task { @MainActor in
            while !Task.isCancelled {
                try? await Task.sleep(nanoseconds: 500_000_000)
                guard !Task.isCancelled else { return }
                checkIdle()
            }
        }
    }

    private func checkIdle() {
        // A pointer that moved without us hearing of it (over the title bar,
        // say) still counts as the viewer being there.
        let pointer = NSEvent.mouseLocation
        if let last = lastPointer, last != pointer,
           playerWindow?.frame.contains(pointer) == true { showControls() }
        lastPointer = pointer

        let now = Date()
        guard player.isPlaying, openMenu == nil, !scrubbing,
              now.timeIntervalSince(lastActivity) >= Self.idleDelay else { return }
        if controlsVisible { controlsVisible = false }
        // A pointer left resting on the seek bar is still "hovering" when the
        // bar fades out, and a view taken away never hears the hover end. The
        // stale position then brought the vignette back with the bar, frozen
        // on an old scene, wherever the pointer had since gone.
        if hoverFraction != nil { hoverFraction = nil }

        // Only when the pointer is over our own window and we are in front:
        // hiding it would otherwise swallow it on another screen or app.
        guard now.timeIntervalSince(lastScroll) >= Self.idleDelay,
              NSApp.isActive, let window = playerWindow, window.isKeyWindow,
              window.frame.contains(pointer) else { return }
        // Self-balancing: the pointer reappears by itself on the next
        // movement, which also brings the controls back.
        NSCursor.setHiddenUntilMouseMoves(true)
    }
}

/// One line in a track list, with the usual menu-style hover highlight.
private struct TrackRow: View {
    let title: String
    let selected: Bool
    let action: () -> Void

    @State private var hovering = false

    var body: some View {
        Button(action: action) {
            HStack(spacing: 8) {
                Image(systemName: "checkmark")
                    .font(.system(size: 10, weight: .bold))
                    .opacity(selected ? 1 : 0)
                Text(title)
                    .font(.system(size: 12))
                    .lineLimit(1)
                Spacer(minLength: 12)
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 5)
            .contentShape(Rectangle())
            .background(RoundedRectangle(cornerRadius: 5)
                .fill(hovering ? Color.primary.opacity(0.12) : .clear)
                .padding(.horizontal, 6))
        }
        .buttonStyle(.plain)
        .onHover { hovering = $0 }
    }
}
