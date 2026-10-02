import Foundation
import VLCKit

/// The last few hundred warnings and errors libvlc emitted, kept in memory and
/// written out only when playback goes wrong.
///
/// A film that freezes mid-playback otherwise leaves no trace at all: VLCKit
/// reports a state change and nothing else, and the reason -- a demuxer giving
/// up on a stretch of file, a hardware decoder losing its session -- is only
/// ever said in libvlc's own log. Logging all of that straight to disk would
/// mean hundreds of megabytes over a three-hour film, so the messages go into
/// a small ring instead and are dumped next to the watch history at the moment
/// something actually breaks.
final class VLCDiagnostics: NSObject, VLCLogging {
    static let shared = VLCDiagnostics()

    /// Errors and warnings only. The debug level is where the volume is, and
    /// none of it says why a playback that was working stopped.
    var level: VLCLogLevel = .warning

    private let lock = NSLock()
    private var ring: [(Date, String)] = []
    private var next = 0
    private static let capacity = 400

    /// Where the dumps go. Kept beside the settings, capped at a megabyte.
    private static var logURL: URL {
        AppSettings.supportDirectory.appendingPathComponent("stalls.log")
    }
    private static let logSizeLimit = 1_000_000

    /// Install before the first player exists, so nothing is missed.
    static func install() {
        VLCLibrary.shared().loggers = [shared]
    }

    // MARK: - Collecting

    /// Called by libvlc, from whichever thread emitted the message.
    func handleMessage(_ message: String, logLevel: VLCLogLevel, context: VLCLogContext?) {
        let module = context?.module ?? "vlc"
        let entry = (Date(), "[\(module)] \(message)")
        lock.lock()
        if ring.count < Self.capacity {
            ring.append(entry)
        } else {
            ring[next] = entry
            next = (next + 1) % Self.capacity
        }
        lock.unlock()
    }

    // MARK: - Dumping

    /// Append everything libvlc has said recently to the log, under a line
    /// saying what went wrong. Off the calling thread: this runs while the
    /// player is busy picking itself back up.
    func dump(reason: String) {
        lock.lock()
        let ordered = ring.count < Self.capacity
            ? ring
            : Array(ring[next...]) + Array(ring[..<next])
        lock.unlock()

        DispatchQueue.global(qos: .utility).async {
            let stamp = Self.formatter.string(from: Date())
            var text = "\n===== \(stamp) — \(reason) =====\n"
            for (date, line) in ordered {
                text += Self.formatter.string(from: date) + " " + line + "\n"
            }
            Self.append(text)
        }
    }

    private static let formatter: DateFormatter = {
        let f = DateFormatter()
        f.dateFormat = "yyyy-MM-dd HH:mm:ss.SSS"
        return f
    }()

    private static func append(_ text: String) {
        let url = logURL
        var existing = (try? Data(contentsOf: url)) ?? Data()
        // Keep the tail: the most recent stall is the one worth reading.
        if existing.count > logSizeLimit {
            existing = existing.suffix(logSizeLimit / 2)
        }
        existing.append(Data(text.utf8))
        try? existing.write(to: url, options: .atomic)
    }
}
