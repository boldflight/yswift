import Foundation
import XCTest
@testable import YSwift

final class YRelativePositionJSONFixtureTests: XCTestCase {
    func testYjsJSONAndBinaryPositionsRoundTrip() throws {
        let url = try XCTUnwrap(Bundle.module.url(forResource: "relative-position-json-v1", withExtension: "json", subdirectory: "Fixtures"))
        let fixture = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(contentsOf: url)) as? [String: Any])
        let positions = try XCTUnwrap(fixture["positions"] as? [[String: Any]])
        for position in positions {
            let name = try XCTUnwrap(position["name"] as? String)
            let expected = try XCTUnwrap(position["json"] as? [String: Any])
            let binary = try XCTUnwrap(Data(base64Encoded: try XCTUnwrap(position["binary"] as? String)))
            XCTAssertEqual(try YDocument.relativePositionBytes(fromJSON: expected), binary, name)
            let projected = try YDocument.relativePositionJSON(from: binary)
            XCTAssertEqual(try JSONSerialization.data(withJSONObject: projected, options: [.sortedKeys]),
                           try JSONSerialization.data(withJSONObject: expected, options: [.sortedKeys]), name)
        }

        XCTAssertEqual(try YDocument.relativePositionBytes(fromJSON: ["tname": "default"]),
                       try XCTUnwrap(Data(base64Encoded: "AQdkZWZhdWx0AA==")))
        XCTAssertThrowsError(try YDocument.relativePositionBytes(fromJSON: ["tname": "default", "assoc": Int64(Int32.max) + 1]))
        XCTAssertThrowsError(try YDocument.relativePositionJSON(from: Data([0xff])))
    }

    func testJSONPositionResolvesOnIntegratedXMLNode() throws {
        let url = try XCTUnwrap(Bundle.module.url(forResource: "relative-ranges-v1", withExtension: "json", subdirectory: "Fixtures"))
        let fixture = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(contentsOf: url)) as? [String: Any])
        let seed = try XCTUnwrap(Data(base64Encoded: try XCTUnwrap(fixture["seed"] as? String)))
        let document = YDocument()
        let root = document.getOrCreateXmlFragment(named: "default")
        try document.transactSync { try $0.transactionApplyUpdate(update: Array(seed)) }
        let text = try XCTUnwrap(root.child(at: 0)?.child(at: 0))
        let binary = try text.relativePosition(at: 1)
        let json = try YDocument.relativePositionJSON(from: binary)
        let restored = try YDocument.relativePositionBytes(fromJSON: json)
        XCTAssertEqual(restored, binary)
        XCTAssertEqual(try document.resolveXmlRelativePosition(restored)?.index, 1)
    }
}
