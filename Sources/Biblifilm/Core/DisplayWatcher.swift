import CoreGraphics
import Foundation

/// Tells the player when a display comes, goes or changes mode.
///
/// The screen this was written for sits on the Mac mini's HDMI port and, on
/// waking, drops and re-raises its hot-plug line about ten times a second for
/// over a minute. A Biblifilm opened in the middle of that played every file
/// with sound over a black picture until the app was restarted: VLC's OpenGL
/// output had been set up against a display that no longer existed.
///
/// AppKit's `didChangeScreenParametersNotification` would be the obvious
/// hook, but it also fires when the Dock hides -- that is, on every switch
/// to full screen -- which would reload the film each time. The CoreGraphics
/// callback only reports real reconfigurations.
enum DisplayWatcher {
    private static var onChange: (() -> Void)?

    /// Start watching. Only the first call counts.
    static func start(_ handler: @escaping () -> Void) {
        guard onChange == nil else { return }
        onChange = handler
        CGDisplayRegisterReconfigurationCallback({ _, flags, _ in
            // Every change is announced twice, before and after; the second
            // one carries what actually happened.
            guard !flags.contains(.beginConfigurationFlag) else { return }
            let relevant: CGDisplayChangeSummaryFlags =
                [.addFlag, .removeFlag, .setModeFlag, .enabledFlag, .disabledFlag]
            guard !flags.intersection(relevant).isEmpty else { return }
            DispatchQueue.main.async { DisplayWatcher.onChange?() }
        }, nil)
    }
}
