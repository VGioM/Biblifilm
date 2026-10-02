import Foundation

/// Walks a folder tree and returns every file VLC can play.
enum LibraryScanner {

    /// Directories that never contain user media.
    private static let skippedDirectories: Set<String> = [
        ".Trash", "$RECYCLE.BIN", "node_modules", ".git", ".svn",
        "System", "Library", "Applications", ".Spotlight-V100", ".fseventsd",
        "@eaDir", "Extras", "Sample", "Samples", "Featurettes"
    ]

    /// Anything smaller than this is a sample/trailer, not a real title.
    private static let minimumVideoBytes: Int64 = 20 * 1024 * 1024   // 20 MB

    struct Options {
        var includeAudio: Bool = true
        var skipSmallFiles: Bool = true
    }

    /// Recursively enumerate `root`. Cancellable via the surrounding Task.
    static func scan(root: URL, options: Options = Options(),
                     progress: (@Sendable (Int) -> Void)? = nil) -> [MediaItem] {
        let fm = FileManager.default
        let keys: [URLResourceKey] = [
            .isDirectoryKey, .isRegularFileKey, .fileSizeKey,
            .contentModificationDateKey, .isPackageKey, .nameKey
        ]

        guard let enumerator = fm.enumerator(
            at: root,
            includingPropertiesForKeys: keys,
            options: [.skipsHiddenFiles, .skipsPackageDescendants]
        ) else { return [] }

        var items: [MediaItem] = []
        var seen = 0

        for case let url as URL in enumerator {
            if Task.isCancelled { return items }

            guard let vals = try? url.resourceValues(forKeys: Set(keys)) else { continue }

            if vals.isDirectory == true {
                if skippedDirectories.contains(url.lastPathComponent) {
                    enumerator.skipDescendants()
                }
                continue
            }
            guard vals.isRegularFile == true else { continue }

            let ext = url.pathExtension.lowercased()
            let isVideo = FilenameParser.videoExtensions.contains(ext)
            let isAudio = FilenameParser.audioExtensions.contains(ext)
            guard isVideo || (isAudio && options.includeAudio) else { continue }

            let size = Int64(vals.fileSize ?? 0)
            if options.skipSmallFiles && isVideo && size < minimumVideoBytes { continue }

            // The parent folder is the best hint when the filename has no show name.
            let parent = url.deletingLastPathComponent().lastPathComponent
            let hint = (parent == root.lastPathComponent) ? nil : parent
            let parsed = FilenameParser.parse(url: url, folderHint: hint)

            items.append(MediaItem(
                url: url,
                parsed: parsed,
                fileSize: size,
                modified: vals.contentModificationDate ?? .distantPast
            ))

            seen += 1
            if seen % 25 == 0 { progress?(seen) }
        }

        progress?(items.count)
        return items.sorted { $0.displayTitle.localizedStandardCompare($1.displayTitle) == .orderedAscending }
    }
}
