import AppKit
import SwiftUI
import VLCKit

/// The VLC render target, extended with the trackpad gesture VLC.app provides:
/// two fingers up/down changes the volume. VLCVideoView itself handles no
/// input at all -- that lives in VLC's own interface, not in VLCKit.
final class GestureVideoView: VLCVideoView {
    var onVolumeStep: ((Int) -> Void)?
    var onToggledPlay: (() -> Void)?
    var onActivity: (() -> Void)?
    var onScroll: (() -> Void)?

    /// One volume step per this many points of scroll, so a normal swipe
    /// moves the volume at roughly the speed VLC does.
    private let pointsPerStep: CGFloat = 12
    private var accumulated: CGFloat = 0

    override var acceptsFirstResponder: Bool { true }
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }

    override func scrollWheel(with event: NSEvent) {
        // A trackpad reports precise deltas; a mouse wheel reports coarse steps.
        let step = event.hasPreciseScrollingDeltas ? pointsPerStep : 1
        accumulated += event.scrollingDeltaY

        while abs(accumulated) >= step {
            let up = accumulated > 0
            accumulated += up ? -step : step
            onVolumeStep?(up ? 5 : -5)
        }
        if event.phase == .ended || event.momentumPhase == .ended { accumulated = 0 }
        // Deliberately no onActivity(): changing the volume should show the
        // volume readout alone, not bring back the whole control bar. The
        // system shows the pointer again on a scroll, though, so say it
        // happened and let the player hide it once the gesture is over.
        onScroll?()
    }

    override func mouseDown(with event: NSEvent) {
        // The first click of a double click has already toggled play/pause by
        // the time the second arrives, so the second toggles it back as well as
        // switching fullscreen: a double click ends up only changing the window.
        if event.clickCount == 2 {
            window?.toggleFullScreen(nil)
        }
        onToggledPlay?()
        onActivity?()
    }

    override func mouseMoved(with event: NSEvent) { onActivity?() }

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        trackingAreas.forEach(removeTrackingArea)
        addTrackingArea(NSTrackingArea(
            rect: bounds,
            options: [.mouseMoved, .activeInKeyWindow, .inVisibleRect],
            owner: self))
    }
}

/// Bridges the VLC view into SwiftUI.
struct VideoSurface: NSViewRepresentable {
    @ObservedObject var player: PlayerModel
    let onActivity: () -> Void
    let onScroll: () -> Void

    func makeNSView(context: Context) -> GestureVideoView {
        let view = GestureVideoView()
        view.backColor = .black
        view.onVolumeStep = { [weak player] delta in player?.nudgeVolume(by: delta) }
        view.onToggledPlay = { [weak player] in player?.togglePlay() }
        view.onActivity = onActivity
        view.onScroll = onScroll
        player.drawable = view
        DispatchQueue.main.async { view.window?.makeFirstResponder(view) }
        return view
    }

    func updateNSView(_ nsView: GestureVideoView, context: Context) {
        nsView.onActivity = onActivity
        nsView.onScroll = onScroll
        if player.drawable !== nsView { player.drawable = nsView }
    }
}
