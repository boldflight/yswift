import Foundation
import Yniffi

/// YDocument holds YSwift shared data types and coordinates collaboration and changes.
public final class YDocument {
    private let document: YrsDoc
    /// Multiple `YDocument` instances are supported. Because `label` is required only for debugging purposes.
    /// It is not used for unique differentiation between queues. So we safely get unique queue for each `YDocument` instance.
    private let transactionQueue = DispatchQueue(label: "YSwift.YDocument", qos: .userInitiated)

    /// Create a new YSwift Document.
    public init() {
        document = YrsDoc()
    }

    /// Compares the state vector from another YSwift document to return a data buffer you can use to synchronize with another YSwift document.
    ///
    /// Use `transactionStateVector()` on a transaction to get a state buffer to compare with this method.
    ///
    /// - Parameters:
    ///   - txn: A transaction within which to compare the state of the document.
    ///   - state: A data buffer from another YSwift document.
    /// - Returns: A buffer that contains the diff you can use to synchronize another YSwift document.
    public func diff(txn: YrsTransaction, from state: [UInt8] = []) -> [UInt8] {
        try! document.encodeDiffV1(tx: txn, stateVector: state)
    }

    // MARK: - Transaction methods

    /// Creates a synchronous transaction and provides that transaction to a trailing closure, within which you make changes to shared data types.
    /// - Parameter changes: The closure in which you make changes to the document.
    /// - Returns: The value that you return from the closure.
    public func transactSync<T>(origin: Origin? = nil, _ changes: @escaping (YrsTransaction) -> T) -> T {
        // Avoiding deadlocks & thread explosion. We do not allow re-entrancy in Transaction methods.
        // It is a programmer's error to invoke synchronous transact from within transaction.
        // Better approach would be to leverage something like `DispatchSpecificKey` in Watchdog style implementation
        // Reference: https://github.com/groue/GRDB.swift/blob/master/GRDB/Core/SchedulingWatchdog.swift
        dispatchPrecondition(condition: .notOnQueue(transactionQueue))
        return transactionQueue.sync {
            let transaction = document.transact(origin: origin?.origin)
            defer {
                transaction.free()
            }
            return changes(transaction)
        }
    }

    /// Runs a throwing edit in one transaction and propagates its error.
    public func transactSync<T>(origin: Origin? = nil, _ changes: @escaping (YrsTransaction) throws -> T) throws -> T {
        try transactSync(origin: origin) { txn in Result { try changes(txn) } }.get()
    }

    /// Creates an asynchronous transaction and provides that transaction to a trailing closure, within which you make changes to shared data types.
    /// - Parameter changes: The closure in which you make changes to the document.
    /// - Returns: The value that you return from the closure.
    public func transact<T>(origin: Origin? = nil, _ changes: @escaping (YrsTransaction) -> T) async -> T {
        await withCheckedContinuation { continuation in
            transactAsync(origin, changes) { result in
                continuation.resume(returning: result)
            }
        }
    }

    /// Creates an asynchronous transaction and provides that transaction to a trailing closure, within which you make changes to shared data types.
    /// - Parameter changes: The closure in which you make changes to the document.
    /// - Parameter completion: A completion handler that is called with the value returned from the closure in which you made changes.
    public func transactAsync<T>(_ origin: Origin? = nil, _ changes: @escaping (YrsTransaction) -> T, completion: @escaping (T) -> Void) {
        transactionQueue.async { [weak self] in
            guard let self = self else { return }
            let transaction = self.document.transact(origin: origin?.origin)
            defer {
                transaction.free()
            }
            let result = changes(transaction)
            completion(result)
        }
    }

    // MARK: - Factory methods

    /// Retrieves or creates a Text shared data type.
    /// - Parameter named: The key you use to reference the Text shared data type.
    /// - Returns: The text shared type.
    public func getOrCreateText(named: String) -> YText {
        YText(text: document.getText(name: named), document: self)
    }

    /// Retrieves the named Y.XmlFragment. RelayMark uses `default`.
    public func getOrCreateXmlFragment(named: String) -> YXmlNode {
        YXmlNode(node: document.getXmlFragment(name: named), document: self)
    }

    /// Creates ephemeral Yjs awareness for this document's client identity.
    public func makeAwareness() -> YAwareness {
        YAwareness(awareness: document.makeAwareness(), document: self)
    }

    /// Reports Yrs updates still waiting for missing predecessor structures or
    /// delete targets. A state vector alone cannot report these pending pieces.
    public func integrationStatus(in transaction: YrsTransaction? = nil) -> YIntegrationStatus {
        if let transaction { return YIntegrationStatus(transaction.integrationStatus()) }
        return transactSync { YIntegrationStatus($0.integrationStatus()) }
    }

    /// Resolves an encoded Yjs relative position to its integrated XML node.
    /// The caller remains responsible for binding the position to its document scope.
    public func resolveXmlRelativePosition(_ encoded: Data, in transaction: YrsTransaction? = nil) throws -> YXmlResolvedPosition? {
        let resolve: (YrsTransaction) throws -> YXmlResolvedPosition? = { txn in
            guard let result = try self.document.resolveXmlRelativePosition(tx: txn, encoded: Array(encoded)) else { return nil }
            return YXmlResolvedPosition(
                node: YXmlNode(node: result.node, document: self),
                index: result.index,
                association: result.association
            )
        }
        if let transaction { return try resolve(transaction) }
        return try transactSync(resolve)
    }

    /// Merges Yjs update-v1 buffers without constructing a document.
    public static func mergeUpdatesV1(_ updates: [Data]) throws -> Data {
        Data(try Yniffi.mergeUpdatesV1(updates: updates.map(Array.init)))
    }

    /// Converts a binary Yjs relative position to its JSON presence shape.
    /// Signed associations are preserved within the Int32 range.
    public static func relativePositionJSON(from encoded: Data) throws -> [String: Any] {
        let json = try Yniffi.relativePositionToJson(encoded: Array(encoded))
        guard let value = try JSONSerialization.jsonObject(with: Data(json.utf8)) as? [String: Any] else {
            throw YRelativePositionError.invalidJSON
        }
        return value
    }

    /// Converts the Yjs JSON presence shape back to canonical binary bytes.
    public static func relativePositionBytes(fromJSON value: [String: Any]) throws -> Data {
        let data = try JSONSerialization.data(withJSONObject: value, options: [.sortedKeys])
        guard let json = String(data: data, encoding: .utf8) else { throw YRelativePositionError.invalidJSON }
        return Data(try Yniffi.relativePositionFromJson(json: json))
    }

    /// Retrieves or creates an Array shared data type.
    /// - Parameter named: The key you use to reference the Array shared data type.
    /// - Returns: The array shared type.
    public func getOrCreateArray<T: Codable>(named: String) -> YArray<T> {
        YArray(array: document.getArray(name: named), document: self)
    }

    /// Retrieves or creates a Map shared data type.
    /// - Parameter named: The key you use to reference the Map shared data type.
    /// - Returns: The map shared type.
    public func getOrCreateMap<T: Codable>(named: String) -> YMap<T> {
        YMap(map: document.getMap(name: named), document: self)
    }

    /// Creates an Undo Manager for a document with the collections that is tracks.
    /// - Parameter trackedRefs: The collections to track to undo and redo changes.
    /// - Returns: A reference to the undo manager to control those actions.
    public func undoManager<T: AnyObject>(trackedRefs: [YCollection]) -> YUndoManager<T> {
        let mapped = trackedRefs.map { $0.pointer() }
        return YUndoManager(manager: document.undoManager(trackedRefs: mapped))
    }
}

public enum YRelativePositionError: Error {
    case invalidJSON
}

public struct YIntegrationStatus: Equatable, Sendable {
    public let hasPendingStructs: Bool
    public let hasPendingDeletes: Bool

    public var isComplete: Bool { !hasPendingStructs && !hasPendingDeletes }

    init(_ value: YrsIntegrationStatus) {
        hasPendingStructs = value.hasPendingStructs
        hasPendingDeletes = value.hasPendingDeletes
    }
}
