import Foundation
import XCTest
@testable import YSwift

final class YAwarenessFixtureTests: XCTestCase {
    private func fixture() throws -> [String: Any] {
        let url = try XCTUnwrap(Bundle.module.url(forResource: "awareness-v1", withExtension: "json", subdirectory: "Fixtures"))
        return try XCTUnwrap(JSONSerialization.jsonObject(with: Data(contentsOf: url)) as? [String: Any])
    }

    private func bytes(_ name: String, from fixture: [String: Any]) throws -> Data {
        try XCTUnwrap(Data(base64Encoded: try XCTUnwrap(fixture[name] as? String)))
    }

    func testYjsAwarenessWireAndClockSemantics() throws {
        let fixture = try fixture()
        let awareness = YDocument().makeAwareness()
        let clientID = try XCTUnwrap(fixture["clientID"] as? UInt64)
        let added = try bytes("added", from: fixture)
        let heartbeat = try bytes("heartbeat", from: fixture)
        let equalRemoval = try bytes("equalClockRemoval", from: fixture)
        let removal = try bytes("removal", from: fixture)

        let first = try XCTUnwrap(awareness.applyUpdate(added))
        XCTAssertEqual(first.added, [clientID])
        XCTAssertEqual(try awareness.state(for: clientID)?.clock, 1)
        XCTAssertEqual((try awareness.state(for: clientID)?.state?["user"] as? [String: String])?["name"], "Ada")
        XCTAssertEqual(try awareness.encodeUpdate(for: [clientID]), added)

        let refreshed = try XCTUnwrap(awareness.applyUpdate(heartbeat))
        XCTAssertEqual(refreshed.updated, [clientID])
        XCTAssertEqual(try awareness.state(for: clientID)?.clock, 2)
        XCTAssertNil(try awareness.applyUpdate(added)) // old clock cannot roll back state
        XCTAssertEqual(try awareness.state(for: clientID)?.clock, 2)

        let gone = try XCTUnwrap(awareness.applyUpdate(removal))
        XCTAssertEqual(gone.removed, [clientID])
        XCTAssertNil(try awareness.state(for: clientID)?.state)
        XCTAssertEqual(try awareness.state(for: clientID)?.clock, 3)
        XCTAssertEqual(try awareness.encodeUpdate(for: [clientID]), removal)
        XCTAssertNil(try awareness.applyUpdate(heartbeat))

        let timedOut = YDocument().makeAwareness()
        try timedOut.applyUpdate(added)
        XCTAssertEqual(try timedOut.removeRemoteState(for: clientID)?.removed, [clientID])
        XCTAssertEqual(try timedOut.encodeUpdate(for: [clientID]), equalRemoval)
        XCTAssertEqual(try timedOut.state(for: clientID)?.clock, 1)
        XCTAssertEqual(try timedOut.applyUpdate(heartbeat)?.updated, [clientID])
        XCTAssertNotNil(try timedOut.state(for: clientID)?.state)
    }

    func testLocalStateAndRemovalInteroperateAcrossPeers() throws {
        let publisher = YDocument().makeAwareness()
        let subscriber = YDocument().makeAwareness()
        try publisher.setLocalState(["user": ["name": "Ada"], "cursor": ["anchor": 1, "head": 3]])
        let initialClock = try XCTUnwrap(publisher.state(for: publisher.clientID)?.clock)
        let first = try publisher.encodeUpdate()
        XCTAssertEqual(try subscriber.applyUpdate(first)?.added, [publisher.clientID])
        XCTAssertEqual((try subscriber.state(for: publisher.clientID)?.state?["user"] as? [String: String])?["name"], "Ada")

        try publisher.setLocalState(try XCTUnwrap(publisher.localState()))
        XCTAssertEqual(try publisher.state(for: publisher.clientID)?.clock, initialClock + 1)
        XCTAssertEqual(try subscriber.applyUpdate(publisher.encodeUpdate())?.updated, [publisher.clientID])

        publisher.clearLocalState()
        let removal = try publisher.encodeUpdate(for: [publisher.clientID])
        XCTAssertEqual(try subscriber.applyUpdate(removal)?.removed, [publisher.clientID])
        XCTAssertNil(try subscriber.state(for: publisher.clientID)?.state)
        XCTAssertNotNil(try subscriber.state(for: publisher.clientID)?.lastUpdatedMillis)
        XCTAssertThrowsError(try subscriber.applyUpdate(Data([0xff])))
    }
}
