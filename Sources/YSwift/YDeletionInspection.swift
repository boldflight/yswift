import Foundation
import Yniffi

extension YDocument {
    /// Whether `after` deletes IDs absent from both `before` and `declaredBy`.
    /// Uses Yrs decoding and range subtraction without mutating replicas.
    /// Reconstructed updates can include cascaded deletes, so a false result
    /// establishes sender observation only when the original update is supplied.
    public static func hasAdditionalDeletionsV1(before: Data, after: Data, declaredBy: Data) throws -> Bool {
        try Yniffi.hasAdditionalDeletionsV1(before: Array(before), after: Array(after), declaredBy: Array(declaredBy))
    }
}
