import AVFoundation
import CoreGraphics
import Foundation
import SwiftUI
import UIKit
internal import VesperPlayerKitBridgeShim
public enum VesperPlayerErrorCode: String, Equatable, Codable {
    case invalidArgument
    case invalidState
    case invalidSource
    case backendFailure
    case audioOutputUnavailable
    case decodeFailure
    case seekFailure
    case unsupported
    case commandChannelClosed
    case eventChannelClosed
    case cancelled
    case timeout
}

public enum VesperPlayerErrorCategory: String, Equatable, Codable {
    case input
    case source
    case network
    case decode
    case audioOutput
    case playback
    case capability
    case platform
}

extension VesperPlayerErrorCode {
    init(ffiCode: PlayerFfiErrorCode) {
        switch ffiCode {
        case PlayerFfiErrorCodeInvalidArgument, PlayerFfiErrorCodeNullPointer,
             PlayerFfiErrorCodeInvalidUtf8, PlayerFfiErrorCodeNone:
            self = .invalidArgument
        case PlayerFfiErrorCodeInvalidState:
            self = .invalidState
        case PlayerFfiErrorCodeInvalidSource:
            self = .invalidSource
        case PlayerFfiErrorCodeBackendFailure:
            self = .backendFailure
        case PlayerFfiErrorCodeAudioOutputUnavailable:
            self = .audioOutputUnavailable
        case PlayerFfiErrorCodeDecodeFailure:
            self = .decodeFailure
        case PlayerFfiErrorCodeSeekFailure:
            self = .seekFailure
        case PlayerFfiErrorCodeUnsupported:
            self = .unsupported
        case PlayerFfiErrorCodeCommandChannelClosed:
            self = .commandChannelClosed
        case PlayerFfiErrorCodeEventChannelClosed:
            self = .eventChannelClosed
        case PlayerFfiErrorCodeCancelled:
            self = .cancelled
        case PlayerFfiErrorCodeTimeout:
            self = .timeout
        default:
            self = .backendFailure
        }
    }

    var ffiCode: PlayerFfiErrorCode {
        switch self {
        case .invalidArgument: return PlayerFfiErrorCodeInvalidArgument
        case .invalidState: return PlayerFfiErrorCodeInvalidState
        case .invalidSource: return PlayerFfiErrorCodeInvalidSource
        case .backendFailure: return PlayerFfiErrorCodeBackendFailure
        case .audioOutputUnavailable: return PlayerFfiErrorCodeAudioOutputUnavailable
        case .decodeFailure: return PlayerFfiErrorCodeDecodeFailure
        case .seekFailure: return PlayerFfiErrorCodeSeekFailure
        case .unsupported: return PlayerFfiErrorCodeUnsupported
        case .commandChannelClosed: return PlayerFfiErrorCodeCommandChannelClosed
        case .eventChannelClosed: return PlayerFfiErrorCodeEventChannelClosed
        case .cancelled: return PlayerFfiErrorCodeCancelled
        case .timeout: return PlayerFfiErrorCodeTimeout
        }
    }
}

extension VesperPlayerErrorCategory {
    init(ffiCategory: PlayerFfiErrorCategory) {
        switch ffiCategory {
        case PlayerFfiErrorCategoryInput:
            self = .input
        case PlayerFfiErrorCategorySource:
            self = .source
        case PlayerFfiErrorCategoryNetwork:
            self = .network
        case PlayerFfiErrorCategoryDecode:
            self = .decode
        case PlayerFfiErrorCategoryAudioOutput:
            self = .audioOutput
        case PlayerFfiErrorCategoryPlayback:
            self = .playback
        case PlayerFfiErrorCategoryCapability:
            self = .capability
        case PlayerFfiErrorCategoryPlatform:
            self = .platform
        default:
            self = .platform
        }
    }

    var ffiCategory: PlayerFfiErrorCategory {
        switch self {
        case .input: return PlayerFfiErrorCategoryInput
        case .source: return PlayerFfiErrorCategorySource
        case .network: return PlayerFfiErrorCategoryNetwork
        case .decode: return PlayerFfiErrorCategoryDecode
        case .audioOutput: return PlayerFfiErrorCategoryAudioOutput
        case .playback: return PlayerFfiErrorCategoryPlayback
        case .capability: return PlayerFfiErrorCategoryCapability
        case .platform: return PlayerFfiErrorCategoryPlatform
        }
    }
}

public struct VesperPlayerError: Error, Equatable {
    public let message: String
    public let code: VesperPlayerErrorCode
    public let category: VesperPlayerErrorCategory
    public let retriable: Bool
    public let details: [String: String]

    public init(
        message: String,
        code: VesperPlayerErrorCode,
        category: VesperPlayerErrorCategory,
        retriable: Bool,
        details: [String: String] = [:]
    ) {
        self.message = message
        self.code = code
        self.category = category
        self.retriable = retriable
        self.details = details
    }
}
