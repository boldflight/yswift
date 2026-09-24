import Foundation
import XCTest
@testable import YSwift

final class YIntegrationStatusTests: XCTestCase {
    private func updates(for id: String) throws -> [Data] {
        let url = try XCTUnwrap(Bundle.module.url(forResource: "convergence-v1", withExtension: "json", subdirectory: "Fixtures"))
        let fixture = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(contentsOf: url)) as? [String: Any])
        let cases = try XCTUnwrap(fixture["cases"] as? [[String: Any]])
        let item = try XCTUnwrap(cases.first { $0["id"] as? String == id })
        return try XCTUnwrap(item["updates"] as? [String]).map { try XCTUnwrap(Data(base64Encoded: $0)) }
    }

    func testPendingDeleteSetClearsWhenInsertionArrives() throws {
        let updates = try updates(for: "unicode-delete-only")
        let document = YDocument()
        XCTAssertTrue(document.integrationStatus().isComplete)
        let pending = try document.transactSync { transaction in
            try transaction.transactionApplyUpdate(update: Array(updates[1]))
            return document.integrationStatus(in: transaction)
        }
        XCTAssertTrue(pending.hasPendingDeletes)
        XCTAssertFalse(pending.hasPendingStructs)
        XCTAssertFalse(document.integrationStatus().isComplete)
        try document.transactSync { try $0.transactionApplyUpdate(update: Array(updates[0])) }
        XCTAssertTrue(document.integrationStatus().isComplete)
    }

    func testPendingStructsClearWhenPredecessorArrives() throws {
        let updates = try updates(for: "concurrent-mark-entity-reference")
        let document = YDocument()
        try document.transactSync { try $0.transactionApplyUpdate(update: Array(updates[1])) }
        XCTAssertTrue(document.integrationStatus().hasPendingStructs)
        try document.transactSync { try $0.transactionApplyUpdate(update: Array(updates[0])) }
        XCTAssertTrue(document.integrationStatus().isComplete)
    }
}
