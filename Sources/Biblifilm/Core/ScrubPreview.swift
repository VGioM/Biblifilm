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
/// position tested came back distinct and correct, in about 0.1s on 1080p
/// H.265 and AV1 and on 4K H.265, and 1-2s on 4K AV1, which only decodes in
/// software. That player draws into memory, never into a window: see
/// FrameGrabber for why that matters.
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

    /// The hidden player the stills come from, and its opening while the
    /// file's shape is being read.
    private var player: FrameGrabber?
    private var opening: Task<FrameGrabber?, Never>?
    private var warmUp: Task<Void, Never>?

    #if DEBUG
    /// The vignette of the player on screen, for the end-to-end tests.
    static weak var active: ScrubPreview?
    #endif

    override init() {
        super.init()
        // ~250 KB a still, so at most ~150 of them, about 40 MB: plenty for
        // the ~240 buckets of a film, which are rarely all visited, and the
        // cache is emptied whenever the player moves to another file anyway.
        cache.countLimit = 150
        cache.totalCostLimit = 40_000_000
    }

    /// Point the previews at a file. Called again as VLC discovers the
    /// duration, and for every following episode; a no-op when nothing moved.
    func configure(url: URL?, duration: Double) {
        #if DEBUG
        Self.active = self
        #endif
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
            let pixels = decoded.representations.first.map { $0.pixelsWide * $0.pixelsHigh } ?? 0
            cache.setObject(decoded, forKey: NSNumber(value: bucket), cost: pixels * 4)
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

    /// Seek the hidden player to `position` and take the first picture it
    /// decodes once it gets there.
    private func grabStill(url: URL, position: Double) async -> NSImage? {
        guard let player = await engine(for: url), !Task.isCancelled else { return nil }
        if !player.isPlaying { player.play() }
        defer { player.pause() }

        let before = player.time
        player.seek(toPosition: position)

        // Wait for the seek to land. The clock having moved at all is part of
        // the test: after a short hop it can already read close to the target
        // while still showing the old picture.
        let target = position * duration * 1000
        var landed = false
        for _ in 0..<100 {
            if Task.isCancelled { return nil }
            let now = player.time
            if now != before && abs(Double(now) - target) < 3000 { landed = true; break }
            try? await Task.sleep(nanoseconds: 30_000_000)
        }
        guard landed else { return nil }

        // Then for pictures decoded from there on: the one VLC had already
        // queued before the clock moved may still belong to the old spot, so
        // the second one is taken. Software-decoded 4K AV1 can take well over
        // a second for that; everything else answers in about 0.1s.
        let start = player.frameCount
        for _ in 0..<200 {
            if Task.isCancelled { return nil }
            if player.frameCount >= start + 2 { break }
            try? await Task.sleep(nanoseconds: 20_000_000)
        }
        guard player.frameCount > start else { return nil }
        return player.latestImage()
    }

    /// The hidden player, built once per file. It draws into memory, not into
    /// a window -- see FrameGrabber -- so it never shows up anywhere, Mission
    /// Control included.
    private func engine(for url: URL) async -> FrameGrabber? {
        if let player { return player }
        if let opening { return await opening.value }

        let task = Task { () -> FrameGrabber? in
            let size = await Self.stillSize(for: url)
            return FrameGrabber(url: url, size: size)
        }
        opening = task
        let grabber = await task.value
        if opening == task { opening = nil }
        // The file may have changed while the size was being read.
        guard self.url == url else { return nil }
        player = grabber
        return grabber
    }

    /// The size to decode the stills at. Big enough to fill the vignette on
    /// a Retina screen (168x94.5 points, cropped to fill), and in the film's
    /// real shape. VLC only tells the picture callbacks the coded size, which
    /// includes the encoder's padding -- 1920x1152 for a 1080p AV1 -- and
    /// ignores anamorphic pixels, so the shape comes from the file's track
    /// list instead: a local parse, about 60ms.
    private static func stillSize(for url: URL) async -> CGSize? {
        let media = VLCMedia(url: url)
        media.parse(options: [], timeout: 2000)
        for _ in 0..<110 where media.parsedStatus != .done {
            try? await Task.sleep(nanoseconds: 20_000_000)
        }
        guard let video = media.tracksInformation
            .compactMap({ $0 as? [String: Any] })
            .first(where: { $0[VLCMediaTracksInformationType] as? String == VLCMediaTracksInformationTypeVideo }),
              let width = (video[VLCMediaTracksInformationVideoWidth] as? NSNumber)?.doubleValue,
              let height = (video[VLCMediaTracksInformationVideoHeight] as? NSNumber)?.doubleValue,
              width > 0, height > 0 else { return nil }

        var aspect = width / height
        if let num = (video[VLCMediaTracksInformationSourceAspectRatio] as? NSNumber)?.doubleValue,
           let den = (video[VLCMediaTracksInformationSourceAspectRatioDenominator] as? NSNumber)?.doubleValue,
           num > 0, den > 0 {
            aspect *= num / den
        }
        guard aspect.isFinite, aspect > 0.2, aspect < 5 else { return nil }

        // Cover 336x189 pixels, the vignette at 2x.
        let target = CGSize(width: 336, height: 189)
        return aspect >= target.width / target.height
            ? CGSize(width: (target.height * aspect).rounded(), height: target.height)
            : CGSize(width: target.width, height: (target.width / aspect).rounded())
    }

    /// Opening the file and showing its first picture is paid here, quietly,
    /// a few seconds into the film, so the first still is as quick as the
    /// rest. The delay keeps the second decoder off the opening scene, where
    /// the film itself is still settling.
    private func scheduleWarmUp() {
        warmUp?.cancel()
        warmUp = Task { [weak self] in
            try? await Task.sleep(nanoseconds: 3_000_000_000)
            guard !Task.isCancelled, let self, let url = self.url,
                  self.player == nil, self.decoding == nil,
                  let player = await self.engine(for: url),
                  !Task.isCancelled, self.decoding == nil else { return }
            player.play()
            for _ in 0..<100 {
                if Task.isCancelled || self.decoding != nil { return }
                if player.frameCount > 0 { break }
                try? await Task.sleep(nanoseconds: 30_000_000)
            }
            // Leave it parked: a paused player costs nothing and is ready to
            // seek the moment the pointer touches the bar.
            if self.decoding == nil { player.pause() }
        }
    }

    private func teardown() {
        warmUp?.cancel()
        warmUp = nil
        opening?.cancel()
        opening = nil
        // Releasing the grabber stops VLC and frees its buffer.
        player = nil
    }
}
