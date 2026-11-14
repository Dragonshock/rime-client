// swift-tools-version: 5.9
import PackageDescription

// Client 不链 librime。Squirrel 编译 sources/，头从 Headers 找，并链接本机 librime。
// Squirrel 链入打开 metal 的 libvoice_ffi.a。归档里同时有 Rust compiler_builtins 与 clang_rt，
// -force_load 会报重复符号。-u 拉住 voice_engine_*，避免 -dead_strip 丢掉 dlsym 符号。
// MLX 的 CPU 对象引用 Accelerate，即使 mlx-rs 没有打开 accelerate 特性。
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
                    "-Xlinker", "-u", "-Xlinker", "_voice_engine_new",
                    "-Xlinker", "-u", "-Xlinker", "_voice_engine_free",
                    "-Xlinker", "-u", "-Xlinker", "_voice_engine_set_callback",
                    "-Xlinker", "-u", "-Xlinker", "_voice_engine_set_reply_sink",
                    "-Xlinker", "-u", "-Xlinker", "_voice_engine_last_error",
                    "-Xlinker", "-u", "-Xlinker", "_voice_session_begin",
                    "-Xlinker", "-u", "-Xlinker", "_voice_session_feed",
                    "-Xlinker", "-u", "-Xlinker", "_voice_session_finish",
                    "-Xlinker", "/Users/dragon/repo/tools/rime-client/code/voice-engine/target/aarch64-apple-darwin/debug/libvoice_ffi.a",
                    "-liconv",
                    "-lc++",
                    "-lobjc",
                    "-framework", "Foundation",
                    "-framework", "Metal",
                    "-framework", "Accelerate",
                    "-lSystem",
                    "-lc",
                    "-lm",
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
