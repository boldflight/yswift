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
    func testRestoredCopiesExcludeIdenticalPeerTextAndRetainDirectUndoEdges() throws {
        let doc=YDocument(guid:"copy-paper",retainDeletedItems:true)
        let root=doc.getOrCreateXmlFragment(named:"default")
        let option=try root.insertElement(named:"questionOption",at:0)
        let text=try option.insertText(at:0)
        try text.insert("A🌍Z",at:0)
        let manager:YUndoManager<NSObject>=doc.undoManager(trackedRefs:[root])
        let origin=Origin("owner");manager.addOrigin(origin)
        let selected=try text.textItems(start:1,length:2)
        try doc.transactSync(origin:origin){try text.removeText(start:1,length:2,in:$0)}
        try text.insert("🌍",at:1)
        let vector=doc.transactSync{Data($0.transactionStateVector())}
        let pre=try JSONSerialization.jsonObject(with:doc.inspectRestoredTextItems(since:vector)) as! [String:Any]
        XCTAssertEqual((pre["copies"] as? [Any])?.count,0)
        XCTAssertTrue(try manager.undo())
        let state=doc.transactSync{Data($0.transactionEncodeStateAsUpdate())}
        let observation=try JSONSerialization.jsonObject(with:doc.inspectRestoredTextItems(since:vector)) as! [String:Any]
        let copies=try XCTUnwrap(observation["copies"] as? [[String:Any]])
        XCTAssertEqual(copies.count,1)
        XCTAssertEqual(try JSONSerialization.data(withJSONObject:copies[0]["source"]!,options:.sortedKeys),try JSONSerialization.data(withJSONObject:JSONSerialization.jsonObject(with:selected),options:.sortedKeys))
        XCTAssertNotEqual(try JSONSerialization.data(withJSONObject:copies[0]["source"]!,options:.sortedKeys),try JSONSerialization.data(withJSONObject:copies[0]["destination"]!,options:.sortedKeys))
        XCTAssertEqual(doc.transactSync{Data($0.transactionEncodeStateAsUpdate())},state)
        XCTAssertThrowsError(try doc.inspectRestoredTextItems(since:vector,maximumSteps:1))
    }

    func testVisibleScopeFindsNewTextAcrossIndependentRichLeaves() throws {
        let doc = YDocument(guid: "visible-paper", retainDeletedItems: true)
        let root = doc.getOrCreateXmlFragment(named: "default")
        let question = try root.insertElement(named: "questionBlock", at: 0)
        let labelNode = try question.insertElement(named: "questionLabel", at: 0)
        let label = try labelNode.insertText(at: 0)
        try label.insert("A🌍Z", at: 0)
        let optionNode = try question.insertElement(named: "questionOption", at: 1)
        let option = try optionNode.insertText(at: 0)
        try option.insert("Choice", at: 0)
        let vector = doc.transactSync { Data($0.transactionStateVector()) }
        try label.insert("!", at: 4)
        try option.insert("🌍", at: 6)
        let state = doc.transactSync { Data($0.transactionEncodeStateAsUpdate()) }
        let result = try JSONSerialization.jsonObject(with: doc.inspectVisibleTextItems(since: vector)) as! [String: Any]
        let runs = try XCTUnwrap(result["runs"] as? [[String: Any]])
        XCTAssertEqual(runs.count, 2)
        XCTAssertEqual(runs.compactMap { $0["count"] as? Int }, [1, 2])
        XCTAssertEqual(runs.compactMap { ($0["item"] as? [String: Any])?["clock"] as? Int }, [15, 16])
        XCTAssertEqual(doc.transactSync { Data($0.transactionEncodeStateAsUpdate()) }, state)
        XCTAssertThrowsError(try doc.inspectVisibleTextItems(since: vector, maximumSteps: 1))
    }

}
