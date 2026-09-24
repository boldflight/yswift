import Foundation
import Yniffi

/// Ephemeral Yjs awareness backed by Yrs. It is not persisted with document updates.
public final class YAwareness {
    private let awareness: YrsAwareness
    private let document: YDocument

    init(awareness: YrsAwareness, document: YDocument) {
        self.awareness = awareness
        self.document = document
    }

    public var clientID: UInt64 { awareness.clientId() }

    /// Replaces the local JSON object and advances its awareness clock.
    /// Calling this with the same state refreshes presence for heartbeat delivery.
    public func setLocalState(_ state: [String: Any]) throws {
        let data = try JSONSerialization.data(withJSONObject: state, options: [.sortedKeys])
        guard let json = String(data: data, encoding: .utf8) else { throw YAwarenessError.invalidState }
        try awareness.setLocalState(json: json)
    }

    /// Advances the local clock and marks this client offline. Encode this
    /// client explicitly afterward to transmit the null state.
    public func clearLocalState() {
        awareness.clearLocalState()
    }

    /// Includes metadata for removed clients so callers can encode tombstones.
    public func states() throws -> [YAwarenessState] {
        try awareness.states().map { value in
            let state: [String: Any]?
            if let json = value.json {
                guard let data = json.data(using: .utf8),
                      let object = try JSONSerialization.jsonObject(with: data) as? [String: Any] else {
                    throw YAwarenessError.invalidState
                }
                state = object
            } else {
                state = nil
            }
            return YAwarenessState(clientID: value.clientId, clock: value.clock,
                                   lastUpdatedMillis: value.lastUpdatedMillis, state: state)
        }
    }

    public func localState() throws -> [String: Any]? {
        try state(for: clientID)?.state
    }

    public func state(for clientID: UInt64) throws -> YAwarenessState? {
        try states().first { $0.clientID == clientID }
    }

    /// Returns bare y-protocols awareness bytes. The nil form includes active
    /// states; pass client IDs to include removed states or a selected subset.
    public func encodeUpdate(for clientIDs: [UInt64]? = nil) throws -> Data {
        if let clientIDs {
            return Data(try awareness.encodeUpdateForClients(clientIds: clientIDs))
        }
        return Data(try awareness.encodeUpdate())
    }

    @discardableResult
    public func applyUpdate(_ encoded: Data) throws -> YAwarenessChanges? {
        try awareness.applyUpdate(encoded: Array(encoded)).map(YAwarenessChanges.init)
    }

    /// Marks a stale remote client offline at its current clock. A later update
    /// with a greater clock can restore it. Callers choose their own timeout.
    @discardableResult
    public func removeRemoteState(for clientID: UInt64) throws -> YAwarenessChanges? {
        try awareness.removeRemoteState(clientId: clientID).map(YAwarenessChanges.init)
    }
}

public struct YAwarenessState {
    public let clientID: UInt64
    public let clock: UInt32
    public let lastUpdatedMillis: UInt64
    public let state: [String: Any]?
}

public struct YAwarenessChanges {
    public let added: [UInt64]
    public let updated: [UInt64]
    public let removed: [UInt64]

    init(_ value: YrsAwarenessChanges) {
        added = value.added
        updated = value.updated
        removed = value.removed
    }
}

public enum YAwarenessError: Error {
    case invalidState
}
