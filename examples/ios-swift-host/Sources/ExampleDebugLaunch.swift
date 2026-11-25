#if DEBUG
import Foundation

/// DEBUG-only launch hook for screenshot and UI verification runs.
///
/// Reads launch arguments such as `-VesperDebugTab library`,
/// `-VesperDebugSheet settings`, and `-VesperDebugCollection dolby` from the
/// argument domain. Each value is consumed once so normal navigation after
/// launch is not affected. Release builds compile none of this.
@MainActor
enum ExampleDebugLaunch {
    private static var consumedKeys: Set<String> = []

    static func consumeTab() -> ExampleHostTab? {
        consume("VesperDebugTab").flatMap(ExampleHostTab.init(rawValue:))
    }

    static func consumeSheet() -> ExampleShellSheet? {
        consume("VesperDebugSheet").flatMap(ExampleShellSheet.init(rawValue:))
    }

    static func consumeCollection() -> ExampleLibraryCollection? {
        consume("VesperDebugCollection").flatMap(ExampleLibraryCollection.init(rawValue:))
    }

    /// `-VesperDebugDetent large` opens the launch sheet at the large detent.
    static func consumeLargeDetent() -> Bool {
        consume("VesperDebugDetent") == "large"
    }

    /// `-VesperDebugPlayerSheet menu` opens a stage playback sheet on launch.
    static func consumePlayerSheet() -> ExamplePlayerSheet? {
        consume("VesperDebugPlayerSheet").flatMap(ExamplePlayerSheet.init(rawValue:))
    }

    /// `-VesperDebugPlayItem live-dvr-acceptance` opens a library item on launch.
    static func consumePlayItem() -> String? {
        consume("VesperDebugPlayItem")
    }

    private static func consume(_ key: String) -> String? {
        guard !consumedKeys.contains(key) else { return nil }
        consumedKeys.insert(key)
        return UserDefaults.standard.volatileDomain(forName: UserDefaults.argumentDomain)[key] as? String
    }
}
#endif
