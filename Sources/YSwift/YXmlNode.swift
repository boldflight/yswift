import Foundation
import Yniffi

/// An integrated Yrs XML fragment, element, or attributed text node.
/// Indices and lengths for XML text are UTF-16 code units; child indices count nodes.
public final class YXmlNode: Transactable, YCollection {
    private let node: YrsXmlNode
    let document: YDocument

    init(node: YrsXmlNode, document: YDocument) {
        self.node = node
        self.document = document
    }

    /// Whether two handles refer to the same integrated XML branch.
    public func isSameNode(as other: YXmlNode) -> Bool {
        document === other.document && node.isSameNode(other: other.node)
    }

    private func withThrowingTransaction<T>(
        _ transaction: YrsTransaction?,
        _ body: @escaping (YrsTransaction) throws -> T
    ) throws -> T {
        try withTransaction(transaction) { txn in Result { try body(txn) } }.get()
    }

    public func pointer() -> YrsCollectionPtr {
        // The document owns the integrated branch for its lifetime.
        withTransaction(nil) { try! self.node.rawPtr(tx: $0) }
    }

    public func kind(in transaction: YrsTransaction? = nil) -> String {
        withTransaction(transaction) { self.node.kind(tx: $0) }
    }

    public func tag(in transaction: YrsTransaction? = nil) -> String? {
        withTransaction(transaction) { self.node.tag(tx: $0) }
    }

    public func length(in transaction: YrsTransaction? = nil) -> UInt32 {
        withTransaction(transaction) { self.node.length(tx: $0) }
    }

    public func child(at index: UInt32, in transaction: YrsTransaction? = nil) -> YXmlNode? {
        withTransaction(transaction) { txn in
            self.node.child(tx: txn, index: index).map { YXmlNode(node: $0, document: self.document) }
        }
    }

    public func children(in transaction: YrsTransaction? = nil) -> [YXmlNode] {
        withTransaction(transaction) { txn in
            (0..<self.node.length(tx: txn)).compactMap { index in
                self.node.child(tx: txn, index: index).map { YXmlNode(node: $0, document: self.document) }
            }
        }
    }

    @discardableResult
    public func insertElement(named tag: String, at index: UInt32, in transaction: YrsTransaction? = nil) throws -> YXmlNode {
        try withThrowingTransaction(transaction) { txn in
            YXmlNode(node: try self.node.insertElement(tx: txn, index: index, tag: tag), document: self.document)
        }
    }

    @discardableResult
    public func insertText(at index: UInt32, in transaction: YrsTransaction? = nil) throws -> YXmlNode {
        try withThrowingTransaction(transaction) { txn in
            YXmlNode(node: try self.node.insertText(tx: txn, index: index), document: self.document)
        }
    }

    public func removeChildren(start: UInt32, length: UInt32, in transaction: YrsTransaction? = nil) throws {
        try withThrowingTransaction(transaction) { try self.node.removeChildren(tx: $0, index: start, length: length) }
    }

    public func attributes(in transaction: YrsTransaction? = nil) throws -> [String: Any] {
        try withThrowingTransaction(transaction) { txn in
            let values = try self.node.attributes(tx: txn)
            return try Dictionary(uniqueKeysWithValues: values.map { attribute in
                guard let data = attribute.valueJson.data(using: .utf8) else { throw YXmlError.invalidAttributes }
                return (attribute.key, try JSONSerialization.jsonObject(with: data, options: [.fragmentsAllowed]))
            })
        }
    }

    public func setAttribute(_ key: String, value: Any, in transaction: YrsTransaction? = nil) throws {
        let data = try JSONSerialization.data(withJSONObject: value, options: [.fragmentsAllowed])
        guard let json = String(data: data, encoding: .utf8) else { throw YXmlError.invalidAttributes }
        try withThrowingTransaction(transaction) { try self.node.setAttribute(tx: $0, key: key, valueJson: json) }
    }

    public func removeAttribute(_ key: String, in transaction: YrsTransaction? = nil) throws {
        try withThrowingTransaction(transaction) { try self.node.removeAttribute(tx: $0, key: key) }
    }

    public func text(in transaction: YrsTransaction? = nil) throws -> String {
        try withThrowingTransaction(transaction) { try self.node.text(tx: $0) }
    }

    public func delta(in transaction: YrsTransaction? = nil) throws -> [YXmlTextRun] {
        try withThrowingTransaction(transaction) { txn in
            try self.node.delta(tx: txn).map { run in
                guard let data = run.attributesJson.data(using: .utf8),
                      let attributes = try JSONSerialization.jsonObject(with: data) as? [String: Any] else {
                    throw YXmlError.invalidAttributes
                }
                return YXmlTextRun(text: run.text, attributes: attributes)
            }
        }
    }

    public func insert(_ text: String, attributes: [String: Any] = [:], at index: UInt32, in transaction: YrsTransaction? = nil) throws {
        let data = try JSONSerialization.data(withJSONObject: attributes)
        guard let json = String(data: data, encoding: .utf8) else { throw YXmlError.invalidAttributes }
        try withThrowingTransaction(transaction) { try self.node.insertString(tx: $0, index: index, value: text, attributesJson: json) }
    }

    public func format(at index: UInt32, length: UInt32, attributes: [String: Any], in transaction: YrsTransaction? = nil) throws {
        let data = try JSONSerialization.data(withJSONObject: attributes)
        guard let json = String(data: data, encoding: .utf8) else { throw YXmlError.invalidAttributes }
        try withThrowingTransaction(transaction) { try self.node.format(tx: $0, index: index, length: length, attributesJson: json) }
    }

    public func removeText(start: UInt32, length: UInt32, in transaction: YrsTransaction? = nil) throws {
        try withThrowingTransaction(transaction) { try self.node.removeText(tx: $0, index: start, length: length) }
    }

    /// Encodes a canonical Yjs relative position for this XML node.
    public func relativePosition(at index: UInt32, association: Int32 = 0, in transaction: YrsTransaction? = nil) throws -> Data {
        try withThrowingTransaction(transaction) { txn in
            Data(try self.node.relativePosition(tx: txn, index: index, association: association))
        }
    }

    /// Returns nil when the position refers to another node or its item is unavailable.
    public func resolveRelativePosition(_ encoded: Data, in transaction: YrsTransaction? = nil) throws -> UInt32? {
        try withThrowingTransaction(transaction) { txn in
            try self.node.resolveRelativePosition(tx: txn, encoded: Array(encoded))
        }
    }
}

public struct YXmlTextRun {
    public let text: String
    public let attributes: [String: Any]
}

public struct YXmlResolvedPosition {
    public let node: YXmlNode
    public let index: UInt32
    public let association: Int32
}

public enum YXmlError: Error {
    case invalidAttributes
}
