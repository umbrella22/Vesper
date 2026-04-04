import Foundation

private final class VesperPlayerI18nBundleToken {}

enum VesperPlayerI18n {
    private static let bundle: Bundle = {
        #if SWIFT_PACKAGE
        return .module
        #else
        return Bundle(for: VesperPlayerI18nBundleToken.self)
        #endif
    }()

    private static func string(_ key: String, _ args: CVarArg...) -> String {
        let format = bundle.localizedString(forKey: key, value: key, table: "Localizable")
        guard !args.isEmpty else { return format }
        return String(format: format, locale: Locale.current, arguments: args)
    }

    static var playerTitle: String { string("vesper.player.title") }
    static var nativeBridgeReady: String { string("vesper.bridge.native.ready") }
    static var previewBridgeReady: String { string("vesper.bridge.preview.ready") }
    static var noSourceSelected: String { string("vesper.bridge.no_source_selected") }
    static var selectSourcePrompt: String { string("vesper.bridge.select_source_prompt") }
    static var invalidMediaUrl: String { string("vesper.bridge.invalid_media_url") }

    static func nativeLocalSourceSubtitle() -> String {
        string("vesper.bridge.native.local_source")
    }

    static func nativeRemoteSourceSubtitle(_ sourceProtocol: String) -> String {
        string("vesper.bridge.native.remote_source", sourceProtocol)
    }

    static func previewLocalSourceSubtitle() -> String {
        string("vesper.bridge.preview.local_source")
    }

    static func previewRemoteSourceSubtitle(_ sourceProtocol: String) -> String {
        string("vesper.bridge.preview.remote_source", sourceProtocol)
    }

    static func nativeBridgeError(_ message: String) -> String {
        string("vesper.bridge.native.error", message)
    }

    static func retryScheduled(delay: String, message: String) -> String {
        string("vesper.bridge.retry_scheduled", delay, message)
    }

    static func retryDelaySecondsInt(_ value: Int) -> String {
        string("vesper.bridge.retry_delay_seconds_int", value)
    }

    static func retryDelaySecondsDecimal(_ value: Double) -> String {
        string("vesper.bridge.retry_delay_seconds_decimal", value)
    }
}
