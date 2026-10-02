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
    func applicationDidFinishLaunching(_ notification: Notification) {
        // Before anything creates a player: libvlc only hands its log to
        // whoever is listening when a message is emitted, and the messages
        // worth having are the ones from the playback that later breaks.
        VLCDiagnostics.install()
        NSApp.setActivationPolicy(.regular)
        NSApp.activate(ignoringOtherApps: true)
        NSApp.appearance = NSAppearance(named: .darkAqua)
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }

    func applicationWillTerminate(_ notification: Notification) {
        // Quitting mid-episode still records where the viewer got to.
        PlayerModel.shared.persistNow()
        // Persist whatever lookups happened this session.
        WatchStore.shared.saveNow()
        // `Task` here would inherit the main actor -- the very thread the wait
        // below blocks -- so the flush could never start and every quit sat
        // out the full two-second timeout with the cache unwritten. Detached,
        // it runs on the concurrent pool and signals in milliseconds.
        let sem = DispatchSemaphore(value: 0)
        Task.detached { await MetadataService.shared.flush(); sem.signal() }
        _ = sem.wait(timeout: .now() + 2)
    }
}
