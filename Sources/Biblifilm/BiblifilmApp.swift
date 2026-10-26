import AppKit
import SwiftUI

@main
struct BiblifilmApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var delegate
    @StateObject private var model = LibraryModel()

    var body: some Scene {
        WindowGroup {
            RootView(model: model)
                .background(Theme.windowBackground)
        }
        .windowToolbarStyle(.unified(showsTitle: true))
        .commands {
            CommandGroup(replacing: .newItem) { }

            CommandGroup(after: .newItem) {
                Button("Choisir un dossier…") { model.chooseFolder() }
                    .keyboardShortcut("o", modifiers: .command)
                Button("Analyser à nouveau") { model.rescan() }
                    .keyboardShortcut("r", modifiers: .command)
                    .disabled(model.libraryPath == nil)
            }

            CommandMenu("Lecture") {
                Button("Lire") {
                    if let item = model.selectedItem { model.play(item) }
                }
                .keyboardShortcut(.return, modifiers: [])
                .disabled(model.selectedItem == nil)

                Button("Afficher dans le Finder") {
                    if let item = model.selectedItem { model.reveal(item) }
                }
                .keyboardShortcut("r", modifiers: [.command, .shift])
                .disabled(model.selectedItem == nil)
            }
        }

        Settings {
            SettingsView(model: model)
                .preferredColorScheme(.dark)
                .tint(Theme.accent)
        }
    }
}

final class AppDelegate: NSObject, NSApplicationDelegate {
    private var signalSources: [DispatchSourceSignal] = []

    func applicationDidFinishLaunching(_ notification: Notification) {
        // Before anything creates a player: libvlc only hands its log to
        // whoever is listening when a message is emitted, and the messages
        // worth having are the ones from the playback that later breaks.
        VLCDiagnostics.install()
        NSApp.setActivationPolicy(.regular)
        NSApp.activate(ignoringOtherApps: true)
        NSApp.appearance = NSAppearance(named: .darkAqua)
        quitCleanlyOnSignals()
        // A Mac put to sleep mid-film may never wake up to write the last
        // seconds: the battery runs out, or it is restarted by hand.
        NSWorkspace.shared.notificationCenter.addObserver(
            forName: NSWorkspace.willSleepNotification, object: nil, queue: .main) { _ in
            MainActor.assumeIsolated { Self.saveWatchState() }
        }
        #if DEBUG
        E2EHarness.installIfRequested()
        #endif
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }

    /// SIGTERM -- a `kill`, or launchd at logout when an app is slow to
    /// answer the quit event -- ends a process on the spot by default, before
    /// anything below has run: the position was whatever the last periodic
    /// save had caught, up to three seconds back. Taken over here, a signal
    /// quits the way ⌘Q does.
    private func quitCleanlyOnSignals() {
        for sig in [SIGTERM, SIGINT, SIGHUP] {
            signal(sig, SIG_IGN)
            let source = DispatchSource.makeSignalSource(signal: sig, queue: .main)
            source.setEventHandler { NSApp.terminate(nil) }
            source.resume()
            signalSources.append(source)
        }
    }

    /// Write the exact position reached, and the history, to disk now.
    @MainActor
    private static func saveWatchState() {
        PlayerModel.shared.persistNow()
        WatchStore.shared.saveNow()
    }

    func applicationWillTerminate(_ notification: Notification) {
        // Quitting mid-episode still records where the viewer got to.
        Self.saveWatchState()
        // `Task` here would inherit the main actor -- the very thread the wait
        // below blocks -- so the flush could never start and every quit sat
        // out the full two-second timeout with the cache unwritten. Detached,
        // it runs on the concurrent pool and signals in milliseconds.
        let sem = DispatchSemaphore(value: 0)
        Task.detached { await MetadataService.shared.flush(); sem.signal() }
        _ = sem.wait(timeout: .now() + 2)
    }
}
