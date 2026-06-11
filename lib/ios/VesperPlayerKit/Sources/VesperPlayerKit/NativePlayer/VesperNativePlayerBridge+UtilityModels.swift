import Foundation

func minimumOptional<T: Comparable>(_ lhs: T?, _ rhs: T?) -> T? {
    switch (lhs, rhs) {
    case let (lhs?, rhs?):
        return min(lhs, rhs)
    case let (lhs?, nil):
        return lhs
    case let (nil, rhs?):
        return rhs
    case (nil, nil):
        return nil
    }
}

func clampToInt64(_ value: Int64) -> Int64 {
    max(value, 0)
}
