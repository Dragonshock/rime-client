// swift-tools-version: 5.9
import PackageDescription

// Client 不链 librime。Squirrel 编译 sources/，头从 Headers 找，并链接本机 librime。
let package = Package(
    name: "Client",
    platforms: [.macOS(.v14)],
    products: [
        .library(name: "Client", targets: ["Client"]),
        .executable(name: "Squirrel", targets: ["Squirrel"]),
    ],
    dependencies: [
        .package(path: "../VoiceInterface"),
        .package(url: "https://github.com/sparkle-project/Sparkle", from: "2.7.1"),
    ],
    targets: [
        .target(
            name: "Client",
            dependencies: [
                .product(name: "VoiceInterface", package: "VoiceInterface"),
            ],
            path: "ClientLib",
            cSettings: [
                .headerSearchPath("../Headers"),
            ]
        ),
        .executableTarget(
            name: "Squirrel",
            dependencies: [
                .product(name: "VoiceInterface", package: "VoiceInterface"),
                .product(name: "Sparkle", package: "Sparkle"),
            ],
            path: "sources",
            exclude: ["Squirrel-Bridging-Header.h"],
            swiftSettings: [
                .unsafeFlags([
                    "-enable-bare-slash-regex",
                    "-import-objc-header",
                    "/Users/dragon/repo/tools/rime-client/code/Client/sources/Squirrel-Bridging-Header.h",
                    "-I", "/Users/dragon/repo/tools/rime-client/code/Client/Headers",
                    "-Xcc", "-I/Users/dragon/repo/tools/rime-client/code/Client/Headers",
                ]),
            ],
            linkerSettings: [
                .unsafeFlags([
                    "-L/nix/store/i3r73fca2jfd0m75xr1zlfmdp3zblhya-librime-1.17.0/lib",
                    "-lrime",
                ]),
            ]
        ),
        .testTarget(
            name: "ClientTests",
            dependencies: [
                "Client",
                .product(name: "VoiceInterface", package: "VoiceInterface"),
            ]
        ),
    ]
)
