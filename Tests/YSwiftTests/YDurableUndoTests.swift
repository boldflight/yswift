import Foundation
import XCTest
@testable import YSwift

final class YDurableUndoTests: XCTestCase {
    private func encode(_ metadata: TestMetadata) throws -> Data { try JSONEncoder().encode(metadata.value) }
    private func decode(_ data: Data) throws -> TestMetadata { TestMetadata(try JSONDecoder().decode(String.self, from: data)) }

    func testDeletedSelectionAndMetadataUndoRedoSurviveReopenAndPeerMerge() throws {
        let document = YDocument(guid: "entity/incarnation", retainDeletedItems: true)
        let text = document.getOrCreateText(named: "default")
        text.insert("alpha beta", at: 0)
        let manager: YUndoManager<TestMetadata> = document.undoManager(trackedRefs: [text])
        manager.addOrigin(Origin("owned-editor"))
        let subscription = manager.observeAdded { _, _ in TestMetadata("selection:6-10") }
        document.transactSync(origin: Origin("owned-editor")) { txn in text.removeRange(start: 6, length: 4, in: txn) }
        let archive = try manager.exportHistory(scopeToken: "account/entity/incarnation/editor", encodeMetadata: encode)
        subscription.cancel()
        XCTAssertEqual(archive.documentGUID, "entity/incarnation")
        XCTAssertTrue(String(decoding: archive.data, as: UTF8.self).contains("selection:6-10"))

        let reopened = YDocument(guid: archive.documentGUID, retainDeletedItems: true)
        let reopenedText = reopened.getOrCreateText(named: "default")
        try reopened.transactSync { txn in try txn.transactionApplyUpdate(update: Array(archive.documentState)) }
        let restored: YUndoManager<TestMetadata> = try reopened.restoreUndoManager(trackedRefs: [reopenedText], history: archive, scopeToken: "account/entity/incarnation/editor", decodeMetadata: decode)
        // The command owner explicitly binds this inverse item; upstream does not
        // infer application operation identity from CRDT state.
        let inverseSubscription = restored.observeAdded { _, _ in TestMetadata("selection:6-10") }
        var popped: String?
        let poppedSubscription = restored.observePopped { _, metadata in popped = metadata?.value; return metadata }
        XCTAssertEqual(restored.undoMetadata().map { $0?.value }, ["selection:6-10"])
        XCTAssertTrue(try restored.undo())
        XCTAssertEqual(reopenedText.getString(), "alpha beta")
        XCTAssertEqual(popped, "selection:6-10")
        let redoArchive = try restored.exportHistory(scopeToken: "account/entity/incarnation/editor", encodeMetadata: encode)
        poppedSubscription.cancel()
        inverseSubscription.cancel()

        let second = YDocument(guid: redoArchive.documentGUID, retainDeletedItems: true)
        let secondText = second.getOrCreateText(named: "default")
        try second.transactSync { txn in try txn.transactionApplyUpdate(update: Array(redoArchive.documentState)) }
        let redoManager: YUndoManager<TestMetadata> = try second.restoreUndoManager(trackedRefs: [secondText], history: redoArchive, scopeToken: "account/entity/incarnation/editor", decodeMetadata: decode)
        XCTAssertEqual(redoManager.redoMetadata().map { $0?.value }, ["selection:6-10"])
        XCTAssertTrue(try redoManager.redo())
        XCTAssertEqual(secondText.getString(), "alpha ")
        let peer = YDocument(guid: "entity/incarnation", retainDeletedItems: true)
        let peerText = peer.getOrCreateText(named: "default")
        exchangeUpdates(second, peer)
        peerText.insert("peer", at: 6)
        exchangeUpdates(second, peer)
        XCTAssertEqual(secondText.getString(), "alpha peer")
        // A checkpoint after a remote merge must still restore the local stack.
        let mergedArchive = try redoManager.exportHistory(scopeToken: "account/entity/incarnation/editor", encodeMetadata: encode)
        let third = YDocument(guid: mergedArchive.documentGUID, retainDeletedItems: true)
        let thirdText = third.getOrCreateText(named: "default")
        try third.transactSync { txn in try txn.transactionApplyUpdate(update: Array(mergedArchive.documentState)) }
        let mergedManager: YUndoManager<TestMetadata> = try third.restoreUndoManager(trackedRefs: [thirdText], history: mergedArchive, scopeToken: "account/entity/incarnation/editor", decodeMetadata: decode)
        XCTAssertTrue(try mergedManager.undo())
        XCTAssertEqual(thirdText.getString(), "alpha betapeer")
        XCTAssertTrue(try mergedManager.redo())
        XCTAssertEqual(thirdText.getString(), "alpha peer")
    }

    func testWrongOwnerAndMissingRetainedDeletesCannotReplaceCurrentDocument() throws {
        let document = YDocument(guid: "paper", retainDeletedItems: true)
        let text = document.getOrCreateText(named: "default")
        let manager: YUndoManager<TestMetadata> = document.undoManager(trackedRefs: [text])
        text.insert("keep", at: 0)
        let archive = try manager.exportHistory(scopeToken: "owner", encodeMetadata: encode)
        XCTAssertThrowsError(try document.restoreUndoManager(trackedRefs: [text], history: archive, scopeToken: "another-owner", decodeMetadata: decode) as YUndoManager<TestMetadata>)
        XCTAssertEqual(text.getString(), "keep")
        XCTAssertTrue(try manager.undo())
        XCTAssertEqual(text.getString(), "")
        let pruned = YDocument(guid: "paper", retainDeletedItems: false)
        let prunedText = pruned.getOrCreateText(named: "default")
        let prunedManager: YUndoManager<TestMetadata> = pruned.undoManager(trackedRefs: [prunedText])
        XCTAssertThrowsError(try prunedManager.exportHistory(scopeToken: "owner", encodeMetadata: encode))
    }
}
