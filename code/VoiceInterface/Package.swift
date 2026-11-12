// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "VoiceInterface",
    platforms: [.macOS(.v14)],
    products: [
        .library(name: "VoiceInterface", targets: ["VoiceInterface"]),
    ],
    targets: [
        .target(name: "VoiceInterface"),
        .testTarget(name: "VoiceInterfaceTests", dependencies: ["VoiceInterface"]),
    ]
)
