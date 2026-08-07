// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "RobineApple",
    platforms: [.iOS(.v17), .macOS(.v14)],
    products: [.library(name: "RobineClient", targets: ["RobineClient"]), .library(name: "RobineUI", targets: ["RobineUI"])],
    targets: [
        .target(name: "RobineClient"),
        .target(name: "RobineUI", dependencies: ["RobineClient"]),
    ]
)
