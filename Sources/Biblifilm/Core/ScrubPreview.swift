import AppKit
import Foundation
import VLCKit

/// The vignette shown while the pointer runs along the seek bar: a still from
/// the film at the position under the cursor, as YouTube and Netflix do.
///
/// A local file has no ready-made sprite sheet, so every still is decoded on
/// demand by VLC. That costs a fraction of a second each, which is why
/// positions are quantised into buckets and cached: sweeping back over the
/// same stretch of the bar is free, and only one decode ever runs at a time
/// so scrubbing cannot pile up work behind the film itself.
///
/// The stills come from a second, hidden VLC player rather than from
/// `VLCMediaThumbnailer`, which this used to use. The thumbnailer is a trap:
/// it pins the decoder to libavcodec, which cannot decode AV1 in this build at
/// all (it starts the codec, hits "avcodec_send_packet critical error" and
/// never answers), and even on files it can read it is unreliable -- measured
/// on one H.265 film, three different positions came back as the same image,
/// byte for byte, and three others came back plain black. A hidden player goes
/// through VLC's normal decoder pick and answers with the right frame: every
/// position tested came back distinct and correct, in about 0.25s on H.265 and
/// 0.5s on AV1.
@MainActor
final class ScrubPreview: NSObject, ObservableObject {

    /// The still to show. It stays on the previous one while the next decodes,
    /// so the vignette never blinks empty in the middle of a scrub.
    @Published private(set) var image: NSImage?

    /// True while `image` is a leftover from another position -- the view dims
    /// it slightly so a frame from a different scene doesn't read as the truth.
    @Published private(set) var isStale = false

    private var url: URL?
    private var duration: Double = 0

    /// Seconds covered by one cached still.
    private var step: Double = 5

    private let cache = NSCache<NSNumber, NSImage>()
    /// Buckets VLC could not decode, so a dud position isn't retried forever.
    private var missing: Set<Int> = []

    /// The bucket the pointer is on, i.e. what we are trying to show.
    private var wanted: Int?
    /// The single decode in flight, if any.
    private var decoding: Int?
    private var job: Task<Void, Never>?
    private var watchdog: Task<Void, Never>?

    /// The hidden player the stills come from, and the offscreen window VLC
    /// needs before it will decode anything at all.
    private var player: VLCMediaPlayer?
    private var window: NSWindow?
    private var warmUp: Task<Void, Never>?

    override init() {
        super.init()
        // 320x180 stills, ~150 of them: a little over 30 MB at worst, and the
        // cache is emptied whenever the player moves to another file anyway.
        cache.countLimit = 150
    }

    /// Point the previews at a file. Called again as VLC discovers the
    /// duration, and for every following episode; a no-op when nothing moved.
    func configure(url: URL?, duration: Double) {
        guard url != self.url || duration != self.duration else { return }

        if url != nil && url == self.url {
            // Same film, VLC has just firmed up its length. The stills decoded
            // so far no longer line up with the new buckets, but the player
            // behind them is still the right one: tearing it down here would
            // pay the cost of opening the file a second time, moments before
            // anyone reaches the seek bar.
            cancelDecode()
            cache.removeAllObjects()
            missing.removeAll()
            wanted = nil
            image = nil
            isStale = false
        } else {
            reset()
        }

        self.url = url
        self.duration = duration
        // Around 240 stills across the film, never finer than five seconds:
        // fine enough that neighbouring positions look different, coarse
        // enough that a slow sweep reuses what is already decoded.
        step = duration > 0 ? max(5, duration / 240) : 5

        if url != nil && duration > 0 && player == nil { scheduleWarmUp() }
    }

    /// Ask for the still at `fraction` of the film; the answer lands on
    /// `image`, either at once from the cache or when VLC is done.
    func request(fraction: Double) {
        guard url != nil, duration > 0 else { return }
        let seconds = min(max(fraction, 0), 1) * duration
        let bucket = Int(seconds / step)
        guard bucket != wanted else { return }
        wanted = bucket

        if let hit = cache.object(forKey: NSNumber(value: bucket)) {
            image = hit
            isStale = false
        } else {
            isStale = image != nil
        }
        pump()
    }

    /// Forget everything: another file, or the player closing.
    func reset() {
        cancelDecode()
        teardown()
        cache.removeAllObjects()
        missing.removeAll()
        wanted = nil
        image = nil
        isStale = false
        url = nil
        duration = 0
    }

    // MARK: - Decoding

    /// Start the decode the pointer is waiting for, if there is one and
    /// nothing else is running.
    private func pump() {
        guard decoding == nil, let bucket = wanted, let url,
              cache.object(forKey: NSNumber(value: bucket)) == nil,
              !missing.contains(bucket) else { return }

        // Aim at the middle of the bucket, and never at the very last frame:
        // the closing black of a film makes for a useless vignette.
        let position = min(0.995, (Double(bucket) * step + step / 2) / duration)
        decoding = bucket

        // A still that never arrives -- a seek that will not land, a stretch
        // of file VLC chokes on -- must not block every later one.
        watchdog = Task { [weak self] in
            try? await Task.sleep(nanoseconds: 14_000_000_000)
            guard !Task.isCancelled else { return }
            self?.finish(bucket, image: nil)
        }

        job = Task { [weak self] in
            guard let self else { return }
            let still = await self.grabStill(url: url, position: position)
            guard !Task.isCancelled else { return }
            self.finish(bucket, image: still)
        }
    }

    private func cancelDecode() {
        watchdog?.cancel()
        watchdog = nil
        job?.cancel()
        job = nil
        decoding = nil
    }

    private func finish(_ bucket: Int, image decoded: NSImage?) {
        guard decoding == bucket else { return }
        cancelDecode()

        if let decoded {
            cache.setObject(decoded, forKey: NSNumber(value: bucket))
            if bucket == wanted {
                image = decoded
                isStale = false
            }
        } else {
            missing.insert(bucket)
        }
        pump()
    }

    // MARK: - The hidden player

    /// Seek the hidden player to `position` and ask VLC for a picture of what
    /// it is showing.
    private func grabStill(url: URL, position: Double) async -> NSImage? {
        let player = engine(for: url)
        if !player.isPlaying { player.play() }

        let before = player.time.value?.doubleValue ?? -1
        player.position = Float(position)
        defer { player.pause() }

        // Wait for the seek to land. Shooting straight away catches the frame
        // VLC has not replaced yet, or a black one while the picture is still
        // on its way -- half the stills came out black before this. The clock
        // having moved at all is part of the test: after a short hop it can
        // already read close to the target while still showing the old frame.
        let target = position * duration * 1000
        for _ in 0..<100 {
            if Task.isCancelled { return nil }
            let now = player.time.value?.doubleValue ?? 0
            if player.hasVideoOut && now != before && abs(now - target) < 3000 { break }
            try? await Task.sleep(nanoseconds: 50_000_000)
        }

        let path = FileManager.default.temporaryDirectory
            .appendingPathComponent("biblifilm-still-\(UUID().uuidString).png").path
        defer { try? FileManager.default.removeItem(atPath: path) }

        // A request made before the video output is ready is dropped without a
        // word, so keep asking until a file shows up. Width alone, so VLC
        // works the height out from the display aspect: an anamorphic film
        // would come out squashed if both were forced.
        for _ in 0..<40 {
            player.saveVideoSnapshot(at: path, withWidth: 320, andHeight: 0)
            try? await Task.sleep(nanoseconds: 100_000_000)
            if Task.isCancelled { return nil }
            if FileManager.default.fileExists(atPath: path) {
                return NSImage(contentsOfFile: path)
            }
        }
        return nil
    }

    /// The hidden player, built once per file. VLC needs somewhere to render
    /// before it will decode at all, hence a real window -- parked far off any
    /// screen, where nobody will ever see it.
    private func engine(for url: URL) -> VLCMediaPlayer {
        if let player { return player }

        // Just under the bottom-left corner of the main display: off every
        // screen, so nobody ever sees it, but a real coordinate rather than
        // the wild -10,000 that used to sit here.
        let origin = NSScreen.main.map { CGPoint(x: $0.frame.minX, y: $0.frame.minY - 400) }
            ?? CGPoint(x: 0, y: -400)
        let window = NSWindow(contentRect: NSRect(origin: origin, size: CGSize(width: 320, height: 180)),
                              styleMask: [.borderless], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        // Mission Control and Expose lay out every ordinary on-screen window an
        // app owns, and this one exists only because VLC will not decode
        // without somewhere to render. Left ordinary it joined that layout as
        // an off-screen outlier, and Mission Control came up with no windows at
        // all or left a thumbnail stuck over the desktop. `.stationary` takes
        // it out of the Expose layout, `.ignoresCycle` out of window cycling,
        // `.fullScreenNone` out of the full-screen machinery, and the desktop
        // level keeps it below everything the window server composites.
        window.collectionBehavior = [.stationary, .ignoresCycle, .fullScreenNone]
        window.level = NSWindow.Level(rawValue: Int(CGWindowLevelForKey(.desktopWindow)))
        window.isExcludedFromWindowsMenu = true
        window.ignoresMouseEvents = true
        window.hasShadow = false
        let view = VLCVideoView(frame: window.contentView!.bounds)
        window.contentView?.addSubview(view)
        window.orderBack(nil)

        let media = VLCMedia(url: url)
        media.addOption(":no-audio")
        let player = VLCMediaPlayer()
        // The render target goes on before play(), never after: the film
        // itself showed what happens when VLC opens a file with nowhere to
        // draw -- it gives up on the picture for the whole playback.
        player.drawable = view
        player.media = media

        self.window = window
        self.player = player
        return player
    }

    /// Opening the file, starting it and landing the first seek costs several
    /// seconds -- measured at 5.5s on one film, against 0.25s for every still
    /// after it. Paid here, quietly, a few seconds into the film, that whole
    /// cost is gone before anyone reaches for the seek bar. The delay keeps
    /// the second decoder off the opening scene, where the film itself is
    /// still settling.
    private func scheduleWarmUp() {
        warmUp?.cancel()
        warmUp = Task { [weak self] in
            try? await Task.sleep(nanoseconds: 3_000_000_000)
            guard !Task.isCancelled, let self, let url = self.url,
                  self.player == nil else { return }
            let player = self.engine(for: url)
            player.play()
            for _ in 0..<100 {
                if Task.isCancelled { return }
                if player.hasVideoOut { break }
                try? await Task.sleep(nanoseconds: 50_000_000)
            }
            // Leave it parked: a paused player costs nothing and is ready to
            // seek the moment the pointer touches the bar.
            player.pause()
        }
    }

    private func teardown() {
        warmUp?.cancel()
        warmUp = nil
        player?.stop()
        player = nil
        // `orderOut` alone leaves the window in the app's list, where the
        // window server still counts it; `close` is what actually retires it.
        window?.orderOut(nil)
        window?.close()
        window = nil
    }
}
