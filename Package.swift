// swift-tools-version:6.0
import PackageDescription

let package = Package(
    name: "Biblifilm",
    platforms: [.macOS(.v14)],
    targets: [
        // Fetched by ./fetch-vlckit.sh -- too large to keep in the project tree.
        .binaryTarget(name: "VLCKit", path: "Frameworks/VLCKit.xcframework"),
        .executableTarget(
            name: "Biblifilm",
            dependencies: ["VLCKit"],
            path: "Sources/Biblifilm",
            swiftSettings: [.swiftLanguageMode(.v5)],
            linkerSettings: [
                .unsafeFlags(["-Xlinker", "-rpath", "-Xlinker", "@executable_path/../Frameworks"])
            ]
        )
    ]
)
