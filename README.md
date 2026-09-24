# YSwift

This library builds on top of [Yrs](https://github.com/y-crdt/y-crdt) to provide Swift language bindings that
seamlessly interoperate with other Yjs implementations.

**This repository is WIP (Work In Progress)**
Not all features and capabilities from Yrs or Yjs are exposed at this time.
We plan to add them as the library evolves.

The repository includes two swift packages:

`yniffiFFI` a static binary packaged as an XCFramework in the `lib` directory, built with the Rust compiler and overlaid using [UniFFI](https://github.com/mozilla/uniffi-rs/).
`YSwift` which is an overlay to provide more idiomatic Swift language operations.

The branch carries the matching `lib/yniffiFFI.xcframework` alongside its Swift
scaffold. A SwiftPM revision dependency therefore uses the same Rust ABI without
requiring Rust in the consuming app build. The binary contains macOS (arm64 and
x86_64), iOS device, iOS simulator (arm64 and x86_64), visionOS device, and
visionOS simulator slices.

To rebuild it from source, install Xcode with the visionOS SDK and Rust with the
`aarch64-apple-darwin`, `x86_64-apple-darwin`, `aarch64-apple-ios`,
`aarch64-apple-ios-sim`, `x86_64-apple-ios`, `aarch64-apple-visionos`, and
`aarch64-apple-visionos-sim` targets. Run `./scripts/build-xcframework.sh` from
the repository root, then `swift test`. The script regenerates the UniFFI Swift
scaffold, compiles each target, assembles the XCFramework, and prints a SHA-256
for its ZIP. Commit the regenerated scaffold and framework together.

This fork exposes Yrs XML fragments, elements and attributed text, Yjs update-v1
merge, and relative positions for use by native editors. XML text offsets use
UTF-16 code units; XML children use node indices. XML handles identify integrated
branches and remain valid across transactions. Deleted branches fail cleanly.

## Decision log

This project maintains a [decision log](./devnotes/DevLog.md).
Please consult it in case there is some ambiguity in terms of why certain implementation details look as they are.

## License

This project is available as open source under the terms of the [MIT License](https://opensource.org/licenses/MIT).

## Thanks to

Amazing people at Mozilla for their outsanding work on [UniFFI](https://github.com/mozilla/uniffi-rs/) and all of the supporting work they've done on using, packaging and distributing Rust code for Swift and Kotlin codebases.
