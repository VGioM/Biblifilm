import AppKit
import Foundation
import VLCKit

/// A VLC player with no window at all: its pictures land in a memory buffer
/// instead of on screen. This is what the seek-bar stills come from.
///
/// VLCKit offers no way to play without somewhere to draw -- its video output
/// insists on an NSView, and `--vout=dummy` is ignored in favour of it -- so
/// the stills used to come from a player drawing into a real window parked
/// off screen. That window was the stuck frame Mission Control left on the
/// desktop, and a second OpenGL surface the window server and the main thread
/// had to keep alive next to the film. libvlc's own "video callbacks" (the
/// vmem output) skip all of it: VLC scales each picture down to the size
/// asked for and writes it into our buffer, on its own thread. Measured on
/// 1080p H.265 it is also much quicker than the window ever was -- first
/// picture in 0.2s instead of 5.5s, about 0.1s per still instead of 0.25s.
///
/// VLCKit does not wrap these callbacks, so the few libvlc functions involved
/// are looked up in the VLCKit binary, which exports the whole C API.
final class FrameGrabber: @unchecked Sendable {

    /// Number of pictures VLC has handed over so far. Lets the caller wait for
    /// a picture that was decoded after a seek, rather than a leftover.
    var frameCount: Int { sink.withLock { $0.frames } }

    /// The playback clock, in milliseconds.
    var time: Int64 { LibVLC.shared.getTime(player) }

    var isPlaying: Bool { LibVLC.shared.isPlaying(player) != 0 }

    private let player: OpaquePointer
    private let sink: Sink

    /// `nil` when libvlc's C API cannot be found or the file cannot be opened.
    ///
    /// `size` is the picture to produce, in pixels. Left out, it is worked out
    /// from the stream once VLC knows it -- but VLC then reports the coded
    /// size, padding included (1920x1152 for a 1080p AV1), so callers that
    /// know the real shape should say so.
    init?(url: URL, size: CGSize?) {
        let vlc = LibVLC.shared
        guard vlc.isAvailable,
              let instance = OpaquePointer(VLCLibrary.shared().instance as UnsafeMutableRawPointer?),
              let media = url.path.withCString({ vlc.mediaNewPath(instance, $0) })
        else { return nil }

        // Pictures only: no sound, no subtitles burnt into the stills.
        for option in [":no-audio", ":no-spu", ":no-sub-autodetect-file"] {
            option.withCString { vlc.mediaAddOption(media, $0) }
        }
        guard let player = vlc.playerNew(media) else {
            vlc.mediaRelease(media)
            return nil
        }
        vlc.mediaRelease(media)

        self.player = player
        self.sink = Sink(size: size)

        let opaque = Unmanaged.passUnretained(sink).toOpaque()
        vlc.setCallbacks(player, Sink.lock, Sink.unlock, Sink.display, opaque)
        vlc.setFormatCallbacks(player, Sink.setup, Sink.cleanup)
    }

    deinit {
        // `stop` waits for VLC's threads to finish, so no callback can touch
        // the buffer once it returns.
        LibVLC.shared.stop(player)
        LibVLC.shared.playerRelease(player)
    }

    func play() { _ = LibVLC.shared.play(player) }
    func pause() { LibVLC.shared.setPause(player, 1) }
    func seek(toPosition position: Double) { LibVLC.shared.setPosition(player, Float(position)) }

    /// The last picture VLC delivered.
    func latestImage() -> NSImage? {
        sink.withLock { state -> NSImage? in
            guard state.frames > 0, let front = state.front, state.width > 0 else { return nil }
            let data = Data(bytes: front, count: state.width * state.height * 4)
            guard let provider = CGDataProvider(data: data as CFData),
                  let image = CGImage(
                    width: state.width, height: state.height,
                    bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: state.width * 4,
                    space: CGColorSpace(name: CGColorSpace.sRGB) ?? CGColorSpaceCreateDeviceRGB(),
                    // VLC's RV32 is B, G, R, padding in memory.
                    bitmapInfo: CGBitmapInfo(rawValue: CGBitmapInfo.byteOrder32Little.rawValue
                                             | CGImageAlphaInfo.noneSkipFirst.rawValue),
                    provider: provider, decode: nil, shouldInterpolate: true,
                    intent: .defaultIntent)
            else { return nil }
            return NSImage(cgImage: image, size: CGSize(width: state.width, height: state.height))
        }
    }
}

// MARK: - The buffer VLC draws into

/// Shared between VLC's video thread and ours. VLC renders into `back`; once
/// a picture is complete it is copied to `front` under the lock, so a still
/// can be read at any moment without ever catching a half-drawn one.
private final class Sink: @unchecked Sendable {
    struct State {
        var width = 0
        var height = 0
        var back: UnsafeMutableRawPointer?
        var front: UnsafeMutableRawPointer?
        var frames = 0
    }

    private let mutex = NSLock()
    private var state = State()
    private let requested: CGSize?

    init(size: CGSize?) { requested = size }

    deinit {
        state.back?.deallocate()
        state.front?.deallocate()
    }

    func withLock<T>(_ body: (inout State) -> T) -> T {
        mutex.lock()
        defer { mutex.unlock() }
        return body(&state)
    }

    private static func from(_ opaque: UnsafeMutableRawPointer?) -> Sink {
        Unmanaged<Sink>.fromOpaque(opaque!).takeUnretainedValue()
    }

    /// VLC announces the stream's format and lets us pick ours: 32-bit RGB at
    /// the size asked for. VLC does the scaling, from whatever the decoder
    /// produced, including hardware-decoded pictures.
    static let setup: LibVLC.SetupCallback = { opaque, chroma, width, height, pitches, lines in
        guard let opaque, let chroma, let width, let height, let pitches, let lines else { return 0 }
        let sink = from(opaque.pointee)

        var w: Int, h: Int
        if let size = sink.requested, size.width >= 2, size.height >= 2 {
            w = Int(size.width); h = Int(size.height)
        } else {
            let sw = max(Int(width.pointee), 1), sh = max(Int(height.pointee), 1)
            w = 320; h = max(2, 320 * sh / sw)
        }
        w -= w % 2; h -= h % 2

        "RV32".withCString { _ = strncpy(chroma, $0, 4) }
        width.pointee = UInt32(w)
        height.pointee = UInt32(h)
        pitches.pointee = UInt32(w * 4)
        lines.pointee = UInt32(h)

        sink.withLock { state in
            if state.width != w || state.height != h {
                state.back?.deallocate()
                state.front?.deallocate()
                state.back = .allocate(byteCount: w * h * 4, alignment: 64)
                state.front = .allocate(byteCount: w * h * 4, alignment: 64)
                state.width = w
                state.height = h
                state.frames = 0
            }
        }
        // One picture is enough: VLC will not write into it again before it
        // has been displayed, which is when we copy it out.
        return 1
    }

    static let cleanup: LibVLC.CleanupCallback = { _ in }

    static let lock: LibVLC.LockCallback = { opaque, planes in
        planes?.pointee = from(opaque).withLock { $0.back }
        return nil
    }

    static let unlock: LibVLC.UnlockCallback = { _, _, _ in }

    static let display: LibVLC.DisplayCallback = { opaque, _ in
        from(opaque).withLock { state in
            guard let back = state.back, let front = state.front else { return }
            front.copyMemory(from: back, byteCount: state.width * state.height * 4)
            state.frames += 1
        }
    }
}

// MARK: - libvlc's C API

/// The handful of libvlc functions VLCKit does not wrap, resolved once from
/// the loaded VLCKit binary.
private struct LibVLC {
    typealias LockCallback = @convention(c) (
        UnsafeMutableRawPointer?, UnsafeMutablePointer<UnsafeMutableRawPointer?>?
    ) -> UnsafeMutableRawPointer?
    typealias UnlockCallback = @convention(c) (
        UnsafeMutableRawPointer?, UnsafeMutableRawPointer?, UnsafePointer<UnsafeMutableRawPointer?>?
    ) -> Void
    typealias DisplayCallback = @convention(c) (UnsafeMutableRawPointer?, UnsafeMutableRawPointer?) -> Void
    typealias SetupCallback = @convention(c) (
        UnsafeMutablePointer<UnsafeMutableRawPointer?>?, UnsafeMutablePointer<CChar>?,
        UnsafeMutablePointer<UInt32>?, UnsafeMutablePointer<UInt32>?,
        UnsafeMutablePointer<UInt32>?, UnsafeMutablePointer<UInt32>?
    ) -> UInt32
    typealias CleanupCallback = @convention(c) (UnsafeMutableRawPointer?) -> Void

    static let shared = LibVLC()

    let isAvailable: Bool
    let mediaNewPath: @convention(c) (OpaquePointer?, UnsafePointer<CChar>) -> OpaquePointer?
    let mediaAddOption: @convention(c) (OpaquePointer?, UnsafePointer<CChar>) -> Void
    let mediaRelease: @convention(c) (OpaquePointer?) -> Void
    let playerNew: @convention(c) (OpaquePointer?) -> OpaquePointer?
    let playerRelease: @convention(c) (OpaquePointer?) -> Void
    let setCallbacks: @convention(c) (OpaquePointer?, LockCallback?, UnlockCallback?,
                                      DisplayCallback?, UnsafeMutableRawPointer?) -> Void
    let setFormatCallbacks: @convention(c) (OpaquePointer?, SetupCallback?, CleanupCallback?) -> Void
    let play: @convention(c) (OpaquePointer?) -> Int32
    let stop: @convention(c) (OpaquePointer?) -> Void
    let setPause: @convention(c) (OpaquePointer?, Int32) -> Void
    let setPosition: @convention(c) (OpaquePointer?, Float) -> Void
    let getTime: @convention(c) (OpaquePointer?) -> Int64
    let isPlaying: @convention(c) (OpaquePointer?) -> Int32

    private init() {
        let handle = UnsafeMutableRawPointer(bitPattern: -2) // RTLD_DEFAULT
        var missing = false
        func load<T>(_ name: String) -> T {
            if let symbol = dlsym(handle, name) { return unsafeBitCast(symbol, to: T.self) }
            missing = true
            return unsafeBitCast(LibVLC.unavailable, to: T.self)
        }
        mediaNewPath = load("libvlc_media_new_path")
        mediaAddOption = load("libvlc_media_add_option")
        mediaRelease = load("libvlc_media_release")
        playerNew = load("libvlc_media_player_new_from_media")
        playerRelease = load("libvlc_media_player_release")
        setCallbacks = load("libvlc_video_set_callbacks")
        setFormatCallbacks = load("libvlc_video_set_format_callbacks")
        play = load("libvlc_media_player_play")
        stop = load("libvlc_media_player_stop")
        setPause = load("libvlc_media_player_set_pause")
        setPosition = load("libvlc_media_player_set_position")
        getTime = load("libvlc_media_player_get_time")
        isPlaying = load("libvlc_media_player_is_playing")
        isAvailable = !missing
    }

    /// Stand-in for a symbol that is not there; never called, since
    /// `isAvailable` is false and FrameGrabber refuses to start.
    private static let unavailable: @convention(c) () -> Void = {}
}
