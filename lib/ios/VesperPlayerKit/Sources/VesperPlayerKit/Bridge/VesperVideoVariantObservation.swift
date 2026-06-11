import AVFoundation
import CoreGraphics
import Foundation
import SwiftUI
import UIKit
import VesperPlayerKitBridgeShim
/// Describes the raw runtime evidence currently observed for the active video
/// variant.
public struct VesperVideoVariantObservation: Equatable {
    public let bitRate: Int64?
    public let width: Int?
    public let height: Int?

    public init(
        bitRate: Int64? = nil,
        width: Int? = nil,
        height: Int? = nil
    ) {
        self.bitRate = bitRate
        self.width = width
        self.height = height
    }
}
