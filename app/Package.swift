// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "Homebase",
    platforms: [.macOS(.v14)],
    targets: [
        .executableTarget(
            name: "Homebase",
            path: "Sources/Homebase"
        ),
        .testTarget(
            name: "HomebaseTests",
            dependencies: ["Homebase"],
            path: "Tests/HomebaseTests"
        ),
    ]
)
