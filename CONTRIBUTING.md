# Contributing to YSwift

This project is a Swift language layer over a binding to the Rust [Yrs](https://github.com/y-crdt/y-crdt) library using Mozilla's [UniFFI](https://github.com/mozilla/uniffi-rs/) project.

## Issues and project work

Issues for `YSwift` are tracked on GitHub at https://github.com/y-crdt/yswift/issues.
We typically keep our roadmap plans within one of those issues, and use issues to loosely track out progress.

This project keeps track of decision points and choices in the library development in a [Developer's Log](./devnotes/DevLog.md), and has a [release process documented](./devnotes/release-process.md).

## Local Development Setup

Scripts in the project build an XCFramework for iOS and macOS using UniFFI from the Rust sources as defined in [lib/Cargo.toml](./lib/Cargo.toml).
The binary included in the released XCFramework is intimately tied to the code that UniFFI generates into `lib/swift/scaffold`.

The rough pattern of dependencies that are reflected in `Package.swift`:

```
   +-----------+      +--------+      +--------+
   | yniffiFFI | <--- | Yniffi | <--- | YSwift |
   +-----------+      +--------+      +--------+
 C static library       UniFFI      Developer created
  from Rust lib        generated      Swift overlay
```

If you are working on any of the layers within the `lib` directory (the targets `yniffiFFI` or `Yniffi`) then set a local environment variable `YSWIFT_LOCAL` to true and regenerate the XCFramework using the script `scripts/build-xcframework.sh`.
The script to build the XCFramework file expects that you have Rust installed locally.
For example, from the roof of the repository:

```bash
export YSWIFT_LOCAL=true
`scripts/build-xcframework.sh`
```

To go back to using the latest released XCFramework, unset the environment variable `YSWIFT_LOCAL`:

```bash
unset YSWIFT_LOCAL
```

The [Package.swift](./Package.swift) file is configured to look for the `YSWIFT_LOCAL` environment variable and use a local reference to the XCFramework if it is set.
Without that environment variable set, `Package.swift` uses the latest release version of the XCFramework.

If you are working on the Swift overlay (`YSwift` target), then you can safely use
the previous released version of the XCFramework and its associated generated code for `Yniffi`.

## Durable owned undo

`YDocument(guid:retainDeletedItems:)` opts into Yrs deleted-item retention. Use a
stable document/incarnation GUID; the CRDT client ID remains independently unique.
`YUndoManager.exportHistory` captures the maintained Yrs stacks and the exact
retained document update. Metadata closures serialize JSON values rather than
process-local object pointers. Reopen the same GUID, load `archive.documentState`,
then call `restoreUndoManager` with the same owner scope and actual tracked
collections before integrating newer peer updates. Export a new checkpoint after
those peer updates. A different GUID, owner, tracked branch, pruned document or
changed state fails admission without mutating the document. The host still owns
account authorization and archive integrity; this local checkpoint is not an
editing grant. Previously GC-pruned deleted content cannot be reconstructed by
restoring stacks.

The binding uses the pinned Yrs `StackItem` serialization and
`Options.init_undo_stack`/`init_redo_stack`; it introduces no parallel undo engine.

## Identity and format observations

`YXmlNode.textItems(start:length:)` exposes real branch/item IDs in UTF-16 runs
using maintained Yrs relative positions. `YDocument.inspectFormatEvent` observes
one exact format-only update against the unchanged prestate through the consumed
fork's `format_event::FormatEvent::inspect`. It returns real marker IDs and values,
including removed markers. Neither API authenticates authorship or receipts.
Hosts must validate the full authored operation and bind evidence to authorized
immutable records. These observation primitives do not claim deleted/redone copy
lineage or attributed replay completeness.
