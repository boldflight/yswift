import Foundation
import XCTest
@testable import YSwift

final class YDeletionInspectionTests: XCTestCase {
    func testActualYjsStructuralDeletionMetadata() throws {
        for (name, operation) in [("paragraph-join-v3", "browser_join_update"),
                                  ("heading-conversion-v3", "browser_conversion_update")] {
            let url = try XCTUnwrap(Bundle.module.url(forResource: name, withExtension: "json", subdirectory: "Fixtures"))
            let fixture = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(contentsOf: url)) as? [String: Any])
            func bytes(_ key: String) throws -> Data {
                try XCTUnwrap(Data(base64Encoded: try XCTUnwrap(fixture[key] as? String)))
            }
            for concurrent in [false, true] {
                let document = YDocument()
                try document.transactSync { try $0.transactionApplyUpdate(update: Array(bytes("seed"))) }
                if concurrent { try document.transactSync { try $0.transactionApplyUpdate(update: Array(bytes("peer_update"))) } }
                let before = Data(document.transactSync { $0.transactionEncodeStateAsUpdate() })
                let incoming = try bytes(operation)
                try document.transactSync { try $0.transactionApplyUpdate(update: Array(incoming)) }
                let after = Data(document.transactSync { $0.transactionEncodeStateAsUpdate() })
                XCTAssertEqual(try YDocument.hasAdditionalDeletionsV1(before: before, after: after, declaredBy: incoming), concurrent, name)
                XCTAssertFalse(try YDocument.hasAdditionalDeletionsV1(before: before, after: after, declaredBy: after), name)
            }
        }
    }

    func testMalformedUpdateThrows() {
        let empty = Data([0, 0]), invalid = Data([255])
        XCTAssertThrowsError(try YDocument.hasAdditionalDeletionsV1(before: invalid, after: empty, declaredBy: empty))
        XCTAssertThrowsError(try YDocument.hasAdditionalDeletionsV1(before: empty, after: invalid, declaredBy: empty))
        XCTAssertThrowsError(try YDocument.hasAdditionalDeletionsV1(before: empty, after: empty, declaredBy: invalid))
    }
}
