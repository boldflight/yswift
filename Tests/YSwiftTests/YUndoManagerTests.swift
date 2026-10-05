import Combine
import XCTest
@testable import YSwift

class TestMetadata {
    public let value: String
    init(_ value: String) {
        self.value = value
    }
}

func exchangeUpdates(_ document1: YDocument, _ document2: YDocument) {
    let clock1 = document1.transactSync { txn in
        txn.transactionStateVector()
    }
    let clock2 = document2.transactSync { txn in
        txn.transactionStateVector()
    }
    let update1 = document1.transactSync { txn in
        document1.diff(txn: txn, from: clock2)
    }
    let update2 = document2.transactSync { txn in
        document2.diff(txn: txn, from: clock1)
    }
    document1.transactSync { txn in
        try! txn.transactionApplyUpdate(update: update2)
    }
    document2.transactSync { txn in
        try! txn.transactionApplyUpdate(update: update1)
    }
}

class YUndoManagerTests: XCTestCase {
    var document: YDocument!
    var text: YText!
    var manager: YUndoManager<TestMetadata>!
    
    override func setUp() {
        document = YDocument()
        text = document.getOrCreateText(named: "test")
        manager = document.undoManager(trackedRefs: [text])
    }
    
    override func tearDown() {
        document = nil
        text = nil
        manager = nil
    }
    
    
    func test_undoTextBasic() throws {
        text.insert("1221", at: 0)
        manager.wrap()
        text.insert("3", at: 2)
        manager.wrap()
        text.insert("3", at: 3)
        manager.wrap()
        
        XCTAssertEqual(text.getString(), "123321")
        
        XCTAssert (try manager.undo())
        XCTAssertEqual(text.getString(), "12321")
        
        XCTAssert (try manager.undo())
        XCTAssertEqual(text.getString(), "1221")
        
        text.insert("3", at: 2)
        XCTAssertEqual(text.getString(), "12321")
    }
    
    func test_undoEvents() throws {
        var received = TestMetadata("")
        
        let on_added = manager.observeAdded({ (e, metadata) -> TestMetadata? in
            let m = metadata ?? TestMetadata("")
            return TestMetadata(m.value + "A")
        })
        
        let on_popped = manager.observePopped({ (e, metadata) -> TestMetadata? in
            received = metadata!
            return metadata
        })
        
        text.insert("abc", at: 0)
        
        XCTAssert (try manager.undo())
        XCTAssertEqual(received.value, "A")
        
        XCTAssert (try manager.redo())
        XCTAssertEqual(received.value, "A")
        
        on_added.cancel()
        on_popped.cancel()
    }
    
    func test_undoDifferentOrigins() throws {
        let localOrigin = Origin("local")
        
        let remoteDocument = YDocument()
        let remoteText = remoteDocument.getOrCreateText(named: "test")
        manager.addOrigin(localOrigin) // only track transaction from local origin
        
        // create some changes locally
        document.transactSync(origin: localOrigin) { txn in
            self.text.insert("hello", at: 0, in: txn)
        }
        self.manager.wrap() // add changes on a stack: they will be undone as one
        
        exchangeUpdates(document, remoteDocument)
        
        // concurrent change on the remote replica
        document.transactSync(origin: localOrigin) { txn in
            self.text.insert(" world", at: 5, in: txn)
        }
        self.manager.wrap() // add next batch of changes on a stack
        remoteText.insert("<break>", at: 1)
        
        XCTAssertEqual(text.getString(), "hello world")
        
        exchangeUpdates(document, remoteDocument)
        
        XCTAssertEqual(text.getString(), "h<break>ello world")
        XCTAssertEqual(remoteText.getString(), "h<break>ello world")
        
        XCTAssertTrue (try manager.undo())
        
        // only changes marked locally have been reversed
        XCTAssertEqual(text.getString(), "h<break>ello")
        
        XCTAssertTrue (try manager.undo())
        XCTAssertEqual(text.getString(), "<break>")
        
        XCTAssertFalse(try manager.undo()) // remote changes are not reverted
        XCTAssertEqual(text.getString(), "<break>")
    }
}


extension YUndoManagerTests {
    func testRetiredWholeItemsReleaseOnlyTheirMetadataAndSurvivorUndoAcrossRedoneChain() throws {
        let doc = YDocument(guid: "retirement-test", retainDeletedItems: true), text = doc.getOrCreateText(named: "default")
        text.insert("A🌍Z", at: 0)
        let owner = Origin("owned"), manager: YUndoManager<TestMetadata> = doc.undoManager(trackedRefs: [text])
        manager.addOrigin(owner)
        var label = "first"
        let added = manager.observeAdded { _, metadata in metadata ?? TestMetadata(label) }
        let popped = manager.observePopped { _, metadata in metadata }
        doc.transactSync(origin: owner) { txn in text.removeRange(start: 0, length: 1, in: txn) }
        weak var retired = manager.undoMetadata()[0]
        manager.wrap(); label = "survivor"
        doc.transactSync(origin: owner) { txn in text.removeRange(start: 0, length: 2, in: txn) }
        XCTAssertTrue(try manager.undo()); XCTAssertTrue(try manager.redo())
        manager.wrap(); label = "redo-retired"
        doc.transactSync(origin: owner) { txn in text.removeRange(start: 0, length: 1, in: txn) }
        XCTAssertTrue(try manager.undo())
        let before = doc.transactSync { doc.diff(txn: $0, from: [0]) }
        XCTAssertThrowsError(try manager.retireHistoryItems(undoIndices: [2], redoIndices: [0]))
        XCTAssertEqual(manager.undoMetadata().compactMap { $0?.value }, ["first", "survivor"])
        XCTAssertEqual(doc.transactSync { doc.diff(txn: $0, from: [0]) }, before)
        let count = try manager.retireHistoryItems(undoIndices: [0], redoIndices: [0])
        XCTAssertEqual(count.undo, 1); XCTAssertEqual(count.redo, 1)
        XCTAssertNil(retired)
        XCTAssertEqual(doc.transactSync { doc.diff(txn: $0, from: [0]) }, before)
        XCTAssertEqual(manager.undoMetadata().compactMap { $0?.value }, ["survivor"])
        XCTAssertTrue(try manager.undo()); XCTAssertEqual(text.getString(), "🌍Z")
        XCTAssertTrue(try manager.redo()); XCTAssertEqual(text.getString(), "Z")
        XCTAssertTrue(try manager.undo()); XCTAssertFalse(try manager.undo())
        doc.transactSync(origin: owner) { txn in text.insert("Ω", at: 3, in: txn) }
        XCTAssertTrue(try manager.undo()); XCTAssertEqual(text.getString(), "🌍Z")
        added.cancel(); popped.cancel()
    }
}
