import Foundation
import Testing
@testable import Biblifilm

/// Mise à jour intégrée (SelfUpdate.swift, identique dans Biblifilm, Biblijeu,
/// Atelier NX et Recomps). Sans réseau : la « release GitHub » est un fichier
/// JSON local, les apps comparées sont de faux bundles dans un dossier jetable.
@Suite("Mise à jour de l'app", .serialized)
struct SelfUpdateTests {

    // MARK: Versions

    @Test func tagsLoseTheirPrefix() {
        #expect(SelfUpdate.clean("v1.3.1") == "1.3.1")
        #expect(SelfUpdate.clean("V2.0") == "2.0")
        #expect(SelfUpdate.clean(" 1.0.0 ") == "1.0.0")
    }

    @Test func versionsCompareNumerically() {
        #expect(SelfUpdate.isNewer("1.3.1", than: "1.3.0"))
        #expect(SelfUpdate.isNewer("1.10.0", than: "1.9.9"))
        #expect(SelfUpdate.isNewer("v2.0", than: "1.99.99"))
        #expect(SelfUpdate.isNewer("1.3.0.1", than: "1.3"))
        #expect(!SelfUpdate.isNewer("1.3.0", than: "1.3.0"))
        #expect(!SelfUpdate.isNewer("1.3", than: "1.3.0"))
        #expect(!SelfUpdate.isNewer("1.2.9", than: "1.3.0"))
    }

    // MARK: Fichier de la release

    @Test func theMacZipIsPicked() {
        let names = ["App-1.0-linux-x86_64.tar.gz", "App-1.0-windows-x64.zip", "App-1.0-macOS-arm64.zip"]
        #expect(SelfUpdate.macZip(among: names) == "App-1.0-macOS-arm64.zip")
    }

    @Test func anUnmarkedZipIsAcceptedButNeverAnotherPlatform() {
        #expect(SelfUpdate.macZip(among: ["App-1.0.zip"]) == "App-1.0.zip")
        #expect(SelfUpdate.macZip(among: ["App-1.0-windows-x64.zip", "App-1.0-linux.zip"]) == nil)
        #expect(SelfUpdate.macZip(among: ["App-1.0.dmg", "notes.txt"]) == nil)
    }

    @Test func notesDropMarkdownAndInstallation() {
        let body = "## Nouveautés\r\n\r\n- **Réglage** des `mises à jour`\r\n\r\n## Installation\r\n\r\n1. Télécharger"
        #expect(SelfUpdate.readableNotes(body) == "Nouveautés\n\n• Réglage des mises à jour")
    }

    @Test func theLatestReleaseIsRead() async throws {
        let dir = try scratch()
        let feed = dir.appendingPathComponent("latest.json")
        try Data("""
        {"tag_name": "v9.8.7", "html_url": "https://github.com/o/r/releases/tag/v9.8.7",
         "body": "## Nouveautés\\n\\n- Une chose",
         "assets": [
           {"name": "App-9.8.7-windows-x64.zip", "browser_download_url": "https://example.invalid/w.zip", "size": 1},
           {"name": "App-9.8.7-macOS-arm64.zip", "browser_download_url": "https://example.invalid/m.zip", "size": 4096}
         ]}
        """.utf8).write(to: feed)

        let release = try await SelfUpdate.latest(repo: "o/r", feed: feed)
        #expect(release.version == "9.8.7")
        #expect(release.download.absoluteString == "https://example.invalid/m.zip")
        #expect(release.size == 4096)
        #expect(release.notes == "Nouveautés\n\n• Une chose")
    }

    @Test func aReleaseWithoutMacZipIsRefused() async throws {
        let feed = try scratch().appendingPathComponent("latest.json")
        try Data("""
        {"tag_name": "v1.0.0", "html_url": "https://github.com/o/r", "body": null,
         "assets": [{"name": "App-1.0.0-linux.tar.gz", "browser_download_url": "https://example.invalid/l", "size": 1}]}
        """.utf8).write(to: feed)
        await #expect(throws: SelfUpdate.Failure.self) {
            _ = try await SelfUpdate.latest(repo: "o/r", feed: feed)
        }
    }

    // MARK: Réglages

    @Test func automaticCheckIsOnByDefaultAndCanBeTurnedOff() {
        let defaults = UserDefaults.standard
        let saved = defaults.object(forKey: SelfUpdate.automaticKey)
        defer { defaults.set(saved, forKey: SelfUpdate.automaticKey) }

        defaults.removeObject(forKey: SelfUpdate.automaticKey)
        #expect(SelfUpdate.automatic)
        SelfUpdate.automatic = false
        #expect(!SelfUpdate.automatic)
        #expect(defaults.bool(forKey: SelfUpdate.automaticKey) == false)
        SelfUpdate.automatic = true
        #expect(SelfUpdate.automatic)
    }

    @Test func aSkippedVersionIsRememberedAndForgotten() {
        let saved = SelfUpdate.skippedVersion
        defer { SelfUpdate.skippedVersion = saved }

        SelfUpdate.skippedVersion = "4.5.6"
        #expect(SelfUpdate.skippedVersion == "4.5.6")
        SelfUpdate.skippedVersion = nil
        #expect(SelfUpdate.skippedVersion == nil)
    }

    @Test func theAutomaticMenuItemFollowsTheSetting() {
        let saved = SelfUpdate.automatic
        defer { SelfUpdate.automatic = saved }

        SelfUpdate.automatic = true
        #expect(SelfUpdate.automaticMenuItem().state == .on)
        SelfUpdate.automatic = false
        #expect(SelfUpdate.automaticMenuItem().state == .off)
    }

    // MARK: Vérifications avant remplacement

    @Test func anotherAppIsRefused() throws {
        let dir = try scratch()
        let current = try fakeApp(in: dir, name: "Old", id: "me.test.app", version: "1.0.0")
        let other = try fakeApp(in: dir, name: "New", id: "me.test.other", version: "2.0.0")
        #expect { try SelfUpdate.verify(other, against: current) } throws: { error in
            if case SelfUpdate.Failure.wrongApp = error { return true }
            return false
        }
    }

    @Test func anOlderOrEqualVersionIsRefused() throws {
        let dir = try scratch()
        let current = try fakeApp(in: dir, name: "Old", id: "me.test.app", version: "1.3.0")
        for (i, v) in ["1.3.0", "1.2.9"].enumerated() {
            let candidate = try fakeApp(in: dir, name: "New\(i)", id: "me.test.app", version: v)
            #expect { try SelfUpdate.verify(candidate, against: current) } throws: { error in
                if case SelfUpdate.Failure.notNewer = error { return true }
                return false
            }
        }
    }

    @Test func anUnsignedNewerAppIsRefused() throws {
        let dir = try scratch()
        let current = try fakeApp(in: dir, name: "Old", id: "me.test.app", version: "1.0.0")
        let candidate = try fakeApp(in: dir, name: "New", id: "me.test.app", version: "1.0.1")
        #expect { try SelfUpdate.verify(candidate, against: current) } throws: { error in
            if case SelfUpdate.Failure.signature = error { return true }
            return false
        }
    }

    @Test func aTranslocatedAppIsNotReplaced() async throws {
        let release = SelfUpdate.Release(version: "9.0.0", notes: "",
                                         page: URL(string: "https://example.invalid")!,
                                         download: URL(string: "https://example.invalid/m.zip")!, size: 0)
        let target = URL(fileURLWithPath: "/private/var/folders/x/AppTranslocation/ABC/d/App.app")
        await #expect { try await SelfUpdate.install(release, replacing: target) } throws: { error in
            if case SelfUpdate.Failure.translocated = error { return true }
            return false
        }
    }

    // MARK: Outils

    private func scratch() throws -> URL {
        let dir = FileManager.default.temporaryDirectory
            .appendingPathComponent("selfupdate-tests-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        return dir
    }

    /// Un bundle réduit à son Info.plist, sans binaire ni signature.
    private func fakeApp(in dir: URL, name: String, id: String, version: String) throws -> URL {
        let app = dir.appendingPathComponent("\(name).app", isDirectory: true)
        let contents = app.appendingPathComponent("Contents", isDirectory: true)
        try FileManager.default.createDirectory(at: contents, withIntermediateDirectories: true)
        let plist: [String: Any] = ["CFBundleIdentifier": id, "CFBundleShortVersionString": version,
                                    "CFBundleName": name, "CFBundlePackageType": "APPL"]
        let data = try PropertyListSerialization.data(fromPropertyList: plist, format: .xml, options: 0)
        try data.write(to: contents.appendingPathComponent("Info.plist"))
        return app
    }
}
