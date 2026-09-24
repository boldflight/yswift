import Foundation
import XCTest
@testable import YSwift

final class YXmlFixtureTests: XCTestCase {
    private func fixture(_ name: String) throws -> [String: Any] {
        let url = try XCTUnwrap(Bundle.module.url(forResource: name, withExtension: "json", subdirectory: "Fixtures"))
        return try XCTUnwrap(JSONSerialization.jsonObject(with: Data(contentsOf: url)) as? [String: Any])
    }

    private func apply(_ encoded: String, to document: YDocument) throws {
        let bytes = try XCTUnwrap(Data(base64Encoded: encoded))
        try document.transactSync { transaction in
            try transaction.transactionApplyUpdate(update: Array(bytes))
        }
    }

    func testYjsFormattedXmlFixture() throws {
        let fixture = try fixture("convergence-v1")
        let cases = try XCTUnwrap(fixture["cases"] as? [[String: Any]])
        let testCase = try XCTUnwrap(cases.first { $0["id"] as? String == "concurrent-mark-entity-reference" })
        let updates = try XCTUnwrap(testCase["updates"] as? [String])
        let document = YDocument()
        let root = document.getOrCreateXmlFragment(named: "default")
        for update in updates { try apply(update, to: document) }

        XCTAssertEqual(root.kind(), "fragment")
        XCTAssertEqual(root.length(), 1)
        let paragraph = try XCTUnwrap(root.child(at: 0))
        XCTAssertEqual(paragraph.tag(), "paragraph")
        let text = try XCTUnwrap(paragraph.child(at: 0))
        let delta = try text.delta()
        XCTAssertEqual(delta.map(\.text), ["Meet ", "Alex", " 🌍", " 👩🏽‍💻"])
        XCTAssertEqual(delta[1].attributes["italic"] as? [String: String], [:])
        XCTAssertEqual((delta[1].attributes["link"] as? [String: String])?["href"],
                       "relay:entity/11111111-1111-4111-8111-111111111111")
        XCTAssertEqual(delta[3].attributes["bold"] as? [String: String], [:])

        let state = document.transactSync { $0.transactionStateVector() }
        let fullUpdate = document.transactSync { $0.transactionEncodeStateAsUpdate() }
        XCTAssertFalse(state.isEmpty)
        let replica = YDocument()
        let replicaRoot = replica.getOrCreateXmlFragment(named: "default")
        try replica.transactSync { try $0.transactionApplyUpdate(update: fullUpdate) }
        let replicaText = try XCTUnwrap(replicaRoot.child(at: 0)?.child(at: 0))
        XCTAssertEqual(try replicaText.delta().map(\.text), delta.map(\.text))
    }

    func testYjsDeleteBeforeInsertAndDuplicateUpdate() throws {
        let fixture = try fixture("convergence-v1")
        let cases = try XCTUnwrap(fixture["cases"] as? [[String: Any]])
        let testCase = try XCTUnwrap(cases.first { $0["id"] as? String == "unicode-delete-only" })
        let updates = try XCTUnwrap(testCase["updates"] as? [String])
        let document = YDocument()
        let root = document.getOrCreateXmlFragment(named: "default")
        for index in [1, 0, 2, 0] { try apply(updates[index], to: document) }
        let text = try XCTUnwrap(root.child(at: 0)?.child(at: 0))
        XCTAssertEqual(try text.delta().map(\.text).joined(), "Aé🇺🇳Z 日本")

        let buffers = try updates.map { try XCTUnwrap(Data(base64Encoded: $0)) }
        let merged = try YDocument.mergeUpdatesV1(buffers)
        let mergedReplica = YDocument()
        let mergedRoot = mergedReplica.getOrCreateXmlFragment(named: "default")
        try mergedReplica.transactSync { try $0.transactionApplyUpdate(update: Array(merged)) }
        let mergedText = try XCTUnwrap(mergedRoot.child(at: 0)?.child(at: 0))
        XCTAssertEqual(try mergedText.text(), "Aé🇺🇳Z 日本")
    }

    func testYjsNumericXmlAttributeAndLocalNull() throws {
        let fixture = try fixture("inline-v1")
        let seed = try XCTUnwrap(fixture["seed"] as? String)
        let document = YDocument()
        let root = document.getOrCreateXmlFragment(named: "default")
        try apply(seed, to: document)
        let heading = try XCTUnwrap(root.child(at: 0))
        XCTAssertEqual(heading.tag(), "heading")
        XCTAssertEqual((try heading.attributes())["level"] as? NSNumber, NSNumber(value: 2))

        try heading.setAttribute("language", value: NSNull())
        XCTAssertTrue((try heading.attributes())["language"] is NSNull)
        try heading.setAttribute("level", value: 3)
        XCTAssertEqual((try heading.attributes())["level"] as? NSNumber, NSNumber(value: 3))
    }

    func testYjsNestedAndRootRelativePositions() throws {
        let fixture = try fixture("relative-ranges-v1")
        let seed = try XCTUnwrap(fixture["seed"] as? String)
        let document = YDocument()
        let root = document.getOrCreateXmlFragment(named: "default")
        try apply(seed, to: document)
        let text = try XCTUnwrap(root.child(at: 0)?.child(at: 0))
        let ranges = try XCTUnwrap(fixture["ranges"] as? [[String: Any]])
        let nested = try XCTUnwrap(ranges.first { $0["name"] as? String == "unicode_nested" })
        let nestedBytes = try XCTUnwrap(nested["range"] as? [String: Any])
        let anchor = try XCTUnwrap(Data(base64Encoded: nestedBytes["anchor"] as? String ?? ""))
        let head = try XCTUnwrap(Data(base64Encoded: nestedBytes["head"] as? String ?? ""))
        XCTAssertEqual(try text.resolveRelativePosition(anchor), 1)
        XCTAssertEqual(try text.resolveRelativePosition(head), 5)
        XCTAssertEqual(try text.relativePosition(at: 1), anchor)
        XCTAssertEqual(try text.relativePosition(at: 5, association: -1), head)
        let resolved = try XCTUnwrap(document.resolveXmlRelativePosition(anchor))
        XCTAssertEqual(resolved.node.kind(), "text")
        XCTAssertTrue(resolved.node.isSameNode(as: text))
        XCTAssertFalse(resolved.node.isSameNode(as: root))
        XCTAssertEqual(resolved.index, 1)
        XCTAssertEqual(resolved.association, 0)

        let boundary = try XCTUnwrap(ranges.first { $0["name"] as? String == "root_boundary" })
        let boundaryBytes = try XCTUnwrap(boundary["range"] as? [String: Any])
        let first = try XCTUnwrap(Data(base64Encoded: boundaryBytes["anchor"] as? String ?? ""))
        let last = try XCTUnwrap(Data(base64Encoded: boundaryBytes["head"] as? String ?? ""))
        XCTAssertEqual(try root.resolveRelativePosition(first), 0)
        XCTAssertEqual(try root.resolveRelativePosition(last), 1)

        let signed = try XCTUnwrap(ranges.first { $0["name"] as? String == "safe_association" })
        let signedBytes = try XCTUnwrap(signed["range"] as? [String: Any])
        let before = try XCTUnwrap(Data(base64Encoded: signedBytes["anchor"] as? String ?? ""))
        let after = try XCTUnwrap(Data(base64Encoded: signedBytes["head"] as? String ?? ""))
        XCTAssertEqual(try text.relativePosition(at: 0, association: -7), before)
        XCTAssertEqual(try text.relativePosition(at: 6, association: 9), after)
        XCTAssertEqual(try text.resolveRelativePosition(before), 0)
        XCTAssertEqual(try text.resolveRelativePosition(after), 6)
        XCTAssertEqual(try document.resolveXmlRelativePosition(before)?.association, -7)
        XCTAssertEqual(try document.resolveXmlRelativePosition(after)?.association, 9)

        let foreign = try XCTUnwrap(ranges.first { $0["name"] as? String == "foreign_nested" })
        let foreignBytes = try XCTUnwrap(foreign["range"] as? [String: Any])
        let unknown = try XCTUnwrap(Data(base64Encoded: foreignBytes["anchor"] as? String ?? ""))
        XCTAssertNil(try text.resolveRelativePosition(unknown))
        XCTAssertNil(try document.resolveXmlRelativePosition(unknown))
    }

    func testXmlUndoTracksLocalOriginAndPreservesRemoteEdit() throws {
        let fixture = try fixture("relative-ranges-v1")
        let seed = try XCTUnwrap(fixture["seed"] as? String)
        let local = YDocument()
        let root = local.getOrCreateXmlFragment(named: "default")
        try apply(seed, to: local)
        let text = try XCTUnwrap(root.child(at: 0)?.child(at: 0))
        let manager: YUndoManager<NSObject> = local.undoManager(trackedRefs: [root])
        let localOrigin = Origin("local-editor")
        manager.addOrigin(localOrigin)
        try local.transactSync(origin: localOrigin) { transaction in
            try text.insert("L", at: 0, in: transaction)
        }
        manager.wrap()

        let remote = YDocument()
        let remoteRoot = remote.getOrCreateXmlFragment(named: "default")
        let localUpdate = local.transactSync { $0.transactionEncodeStateAsUpdate() }
        try remote.transactSync { try $0.transactionApplyUpdate(update: localUpdate) }
        let remoteText = try XCTUnwrap(remoteRoot.child(at: 0)?.child(at: 0))
        try remoteText.insert("R", at: remoteText.length())
        let remoteUpdate = remote.transactSync { $0.transactionEncodeStateAsUpdate() }
        try local.transactSync { try $0.transactionApplyUpdate(update: remoteUpdate) }
        let beforeUndo = try text.text()
        XCTAssertTrue(beforeUndo.hasPrefix("L"))
        XCTAssertTrue(beforeUndo.hasSuffix("R"))
        XCTAssertTrue(try manager.undo())
        let afterUndo = try text.text()
        XCTAssertFalse(afterUndo.hasPrefix("L"))
        XCTAssertTrue(afterUndo.hasSuffix("R"))
    }
}
