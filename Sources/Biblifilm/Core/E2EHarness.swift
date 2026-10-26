#if DEBUG
import AppKit
import Foundation

/// Remote control for the end-to-end tests (Tests/E2E/run.sh). Debug builds
/// only: the release app never listens for any of this.
///
/// `BIBLIFILM_E2E_PLAY` lists the files to open on launch, separated by "|":
/// the first plays, the rest queue behind it, as a season would. Commands
/// then arrive as distributed notifications, so the script can act like a
/// viewer -- skip, leave, quit -- without needing accessibility rights.
@MainActor
enum E2EHarness {
    static let commandNotification = Notification.Name("com.biblifilm.e2e.command")

    private static var observer: NSObjectProtocol?

    static func installIfRequested() {
        let env = ProcessInfo.processInfo.environment
        guard let list = env["BIBLIFILM_E2E_PLAY"], !list.isEmpty else { return }

        observer = DistributedNotificationCenter.default().addObserver(
            forName: commandNotification, object: nil, queue: .main) { note in
            let command = note.object as? String ?? ""
            MainActor.assumeIsolated { perform(command) }
        }

        let items = list.split(separator: "|").map { path -> MediaItem in
            let url = URL(fileURLWithPath: String(path))
            return MediaItem(url: url,
                             parsed: FilenameParser.parse(url: url),
                             fileSize: 0, modified: .now)
        }
        guard let first = items.first else { return }
        // Let SwiftUI put the window up first, as a click would.
        DispatchQueue.main.asyncAfter(deadline: .now() + 1) {
            PlayerModel.shared.open(first, following: Array(items.dropFirst()))
        }
        writeState()
    }

    private static func perform(_ command: String) {
        let player = PlayerModel.shared
        let parts = command.split(separator: " ")
        switch parts.first {
        case "next":        player.skipToNext()
        case "close":       player.close()
        case "pause":       if player.isPlaying { player.togglePlay() }
        case "seek":        if parts.count > 1, let s = Double(parts[1]), player.duration > 0 {
                                player.seek(toFraction: s / player.duration)
                            }
        case "terminate":   NSApp.terminate(nil)
        case "cmdq":        pressCommandQ()
        case "fullscreen":  PlayerModel.shared.drawable?.window?.toggleFullScreen(nil)
        case "closeWindow": NSApp.windows.first { $0.isVisible && $0.canBecomeMain }?.performClose(nil)
        default:            break
        }
    }

    /// A real ⌘Q keystroke, delivered through the event queue so it takes
    /// the same road as one typed on the keyboard: past the player's key
    /// monitor and into the main menu's key equivalents.
    private static func pressCommandQ() {
        let window = NSApp.keyWindow ?? NSApp.windows.first { $0.isVisible }
        for type in [NSEvent.EventType.keyDown, .keyUp] {
            guard let event = NSEvent.keyEvent(
                with: type, location: .zero, modifierFlags: .command,
                timestamp: ProcessInfo.processInfo.systemUptime,
                windowNumber: window?.windowNumber ?? 0, context: nil,
                characters: "q", charactersIgnoringModifiers: "q",
                isARepeat: false, keyCode: 12) else { continue }
            NSApp.postEvent(event, atStart: false)
        }
    }

    /// The player as the script sees it, refreshed every half second.
    private static func writeState() {
        Task { @MainActor in
            while true {
                let player = PlayerModel.shared
                let state: [String: Any] = [
                    "current": player.current?.id ?? "",
                    "time": player.time,
                    "duration": player.duration,
                    "isPlaying": player.isPlaying,
                    "canPlayNext": player.canPlayNext,
                ]
                let url = AppSettings.supportDirectory.appendingPathComponent("e2e-state.json")
                if let data = try? JSONSerialization.data(withJSONObject: state) {
                    try? data.write(to: url, options: .atomic)
                }
                try? await Task.sleep(nanoseconds: 500_000_000)
            }
        }
    }
}
#endif
