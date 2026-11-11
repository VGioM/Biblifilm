import SwiftUI

/// Section « Mises à jour de l'app » des réglages, branchée sur SelfUpdate.
/// Le même fichier vit dans Biblifilm et Biblijeu.
struct SelfUpdateSettings: View {
    @AppStorage(SelfUpdate.automaticKey) private var automatic = true
    @State private var skipped = SelfUpdate.skippedVersion

    var body: some View {
        Section("Mises à jour de \(SelfUpdate.appName)") {
            Toggle(SelfUpdate.automaticTitle, isOn: $automatic)
            Text("Au lancement, une fois par jour au plus. Chaque nouvelle version est proposée avant d'être installée : « Mettre à jour », « Plus tard » ou « Ignorer cette version ».")
                .font(.caption).foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
            LabeledContent("Version installée") {
                Text(SelfUpdate.currentVersion).foregroundStyle(.secondary).monospacedDigit()
            }
            if let skipped {
                LabeledContent("Version ignorée") {
                    HStack {
                        Text(skipped).foregroundStyle(.secondary).monospacedDigit()
                        Button("Ne plus l'ignorer") {
                            SelfUpdate.skippedVersion = nil
                            self.skipped = nil
                        }
                    }
                }
            }
            Button(SelfUpdate.menuTitle) { SelfUpdate.checkNow() }
        }
        .onAppear { skipped = SelfUpdate.skippedVersion }
    }
}
