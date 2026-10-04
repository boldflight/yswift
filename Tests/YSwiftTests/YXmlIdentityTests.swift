import Foundation
import XCTest
@testable import YSwift

final class YXmlIdentityTests: XCTestCase {
    func testUnicodeTextItemsAndCausalFormatEventsUseSameRetainedOwner() throws {
        let document = YDocument(guid: "paper", retainDeletedItems: true)
        let root = document.getOrCreateXmlFragment(named: "default")
        let paragraph = try root.insertElement(named: "paragraph", at: 0)
        let text = try paragraph.insertText(at: 0)
        try text.insert("A🌍Z", at: 0)
        let before = document.transactSync { Data($0.transactionEncodeStateAsUpdate()) }
        let targets = try text.textItems(start: 1, length: 2)
        let runs = try XCTUnwrap((JSONSerialization.jsonObject(with: targets) as? [String: Any])?["runs"] as? [[String: Any]])
        XCTAssertEqual(runs.count, 1)
        XCTAssertEqual(runs[0]["count"] as? Int, 2)
        let peer = YDocument(guid: "paper", retainDeletedItems: true)
        let peerRoot = peer.getOrCreateXmlFragment(named: "default")
        try peer.transactSync { try $0.transactionApplyUpdate(update: Array(before)) }
        let peerText = try XCTUnwrap(peerRoot.child(at: 0)?.child(at: 0))
        XCTAssertEqual(try peerText.textItems(start: 1, length: 2), targets)
        let update = try peer.transactSync { txn -> Data in
            try peerText.format(at: 1, length: 2, attributes: ["bold": [String: Any]()], in: txn)
            return Data(txn.transactionEncodeUpdate())
        }
        let observation = try document.inspectFormatEvent(update: update, attribute: "bold", textItems: targets)
        let event = try XCTUnwrap(JSONSerialization.jsonObject(with: observation) as? [String: Any])
        let changes = try XCTUnwrap(event["changes"] as? [[String: Any]])
        XCTAssertEqual(changes.count, 2)
        XCTAssertNotNil((changes[0]["after"] as? [String: Any])?["marker"] as? [String: Any])
        XCTAssertEqual(document.transactSync { Data($0.transactionEncodeStateAsUpdate()) }, before)
        XCTAssertThrowsError(try document.inspectFormatEvent(update: Data([255]), attribute: "bold", textItems: targets))
        XCTAssertThrowsError(try text.textItems(start: 4, length: 1))
        let foreign = try paragraph.insertText(at: 1)
        try foreign.insert("XY", at: 0)
        XCTAssertThrowsError(try document.inspectFormatEvent(update: update, attribute: "bold", textItems: foreign.textItems(start: 0, length: 2)))
    }
}
