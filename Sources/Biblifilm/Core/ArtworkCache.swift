import AppKit
import Foundation

/// Downloads cover art once and keeps it on disk, so the grid is instant on relaunch.
actor ArtworkCache {
    static let shared = ArtworkCache()

    private let directory: URL
    private var memory = NSCache<NSString, NSImage>()
    private var inFlight: [String: Task<NSImage?, Never>] = [:]

    init() {
        directory = AppSettings.supportDirectory.appendingPathComponent("Artwork", isDirectory: true)
        try? FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        memory.countLimit = 400
    }

    private func filename(for urlString: String) -> URL {
        // Stable, filesystem-safe name derived from the remote URL.
        var hash: UInt64 = 5381
        for byte in urlString.utf8 { hash = (hash &* 33) &+ UInt64(byte) }
        return directory.appendingPathComponent(String(hash, radix: 36) + ".img")
    }

    func image(for urlString: String?) async -> NSImage? {
        guard let urlString, let remote = URL(string: urlString) else { return nil }

        if let hit = memory.object(forKey: urlString as NSString) { return hit }

        let file = filename(for: urlString)
        if let data = try? Data(contentsOf: file), let img = NSImage(data: data) {
            memory.setObject(img, forKey: urlString as NSString)
            return img
        }

        if let running = inFlight[urlString] { return await running.value }

        let task = Task<NSImage?, Never> { [file] in
            guard let (data, response) = try? await HTTP.session.data(from: remote),
                  let http = response as? HTTPURLResponse, (200...299).contains(http.statusCode),
                  let img = NSImage(data: data) else { return nil }
            try? data.write(to: file, options: .atomic)
            return img
        }
        inFlight[urlString] = task
        let result = await task.value
        inFlight[urlString] = nil

        if let result { memory.setObject(result, forKey: urlString as NSString) }
        return result
    }

    func clear() {
        memory.removeAllObjects()
        try? FileManager.default.removeItem(at: directory)
        try? FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
    }

    func diskSize() -> Int64 {
        guard let files = try? FileManager.default.contentsOfDirectory(
            at: directory, includingPropertiesForKeys: [.fileSizeKey]) else { return 0 }
        return files.reduce(Int64(0)) { total, url in
            total + Int64((try? url.resourceValues(forKeys: [.fileSizeKey]).fileSize) ?? 0)
        }
    }
}
