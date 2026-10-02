import CoreServices
import Foundation

/// Watches a folder tree and reports changes, so files added after launch
/// show up on their own. Backed by FSEvents, which is recursive.
final class FolderWatcher {
    private var stream: FSEventStreamRef?
    private var handler: (() -> Void)?

    deinit { stopStream() }

    /// Begin watching `path`. Replaces any previous watch.
    func start(path: String, latency: TimeInterval = 2.0, handler: @escaping () -> Void) {
        stop()
        self.handler = handler

        var context = FSEventStreamContext(
            version: 0,
            info: Unmanaged.passUnretained(self).toOpaque(),
            retain: nil, release: nil, copyDescription: nil)

        let callback: FSEventStreamCallback = { _, info, _, _, _, _ in
            guard let info else { return }
            Unmanaged<FolderWatcher>.fromOpaque(info).takeUnretainedValue().handler?()
        }

        let flags = FSEventStreamCreateFlags(
            kFSEventStreamCreateFlagFileEvents | kFSEventStreamCreateFlagNoDefer)

        guard let created = FSEventStreamCreate(
            kCFAllocatorDefault, callback, &context,
            [path] as CFArray,
            FSEventStreamEventId(kFSEventStreamEventIdSinceNow),
            latency, flags) else { return }

        FSEventStreamSetDispatchQueue(created, DispatchQueue.main)
        FSEventStreamStart(created)
        stream = created
    }

    func stop() { stopStream() }

    private func stopStream() {
        guard let stream else { return }
        FSEventStreamStop(stream)
        FSEventStreamInvalidate(stream)
        FSEventStreamRelease(stream)
        self.stream = nil
    }
}
