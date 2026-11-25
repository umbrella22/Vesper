import SwiftUI
import XCTest
@testable import VesperPlayerHostDemo

final class ExampleShellModelTests: XCTestCase {
    func testTabsKeepDisplayOrderAndDistinctIcons() {
        XCTAssertEqual(ExampleHostTab.allCases, [.play, .library, .downloads, .lab])
        let icons = ExampleHostTab.allCases.map(\.systemImage)
        XCTAssertEqual(Set(icons).count, icons.count)
        for tab in ExampleHostTab.allCases {
            XCTAssertFalse(tab.title.isEmpty, tab.rawValue)
        }
    }

    func testShellSheetsStartAtMediumAndCanExpand() {
        for sheet in [ExampleShellSheet.openMedia, .settings, .newDownload] {
            let detents = exampleShellSheetDetents(sheet)
            XCTAssertTrue(detents.contains(.medium), sheet.rawValue)
            XCTAssertTrue(detents.contains(.large), sheet.rawValue)
        }
    }

    func testPlayerSheetDetentsKeepShortListsAtMedium() {
        XCTAssertEqual(examplePlayerSheetDetents(.menu), [.medium])
        XCTAssertEqual(examplePlayerSheetDetents(.speed), [.medium])
        for sheet in [ExamplePlayerSheet.quality, .audio, .subtitle] {
            XCTAssertEqual(examplePlayerSheetDetents(sheet), [.medium, .large])
        }
    }

    func testMiniPlayerOnlyShowsAwayFromPlayWithActiveSourceAndNotFullscreen() {
        XCTAssertFalse(exampleShowsMiniPlayer(selectedTab: .play, hasActiveSource: true, isFullscreen: false))
        XCTAssertFalse(exampleShowsMiniPlayer(selectedTab: .library, hasActiveSource: false, isFullscreen: false))
        XCTAssertFalse(exampleShowsMiniPlayer(selectedTab: .downloads, hasActiveSource: true, isFullscreen: true))
        for tab in [ExampleHostTab.library, .downloads, .lab] {
            XCTAssertTrue(
                exampleShowsMiniPlayer(selectedTab: tab, hasActiveSource: true, isFullscreen: false),
                tab.rawValue
            )
        }
    }

    func testLibraryCollectionsMapToKnownPlaylistItems() {
        XCTAssertEqual(exampleLibraryItems(for: .vod).map(\.id), [IOS_HLS_PLAYLIST_ITEM_ID])
        XCTAssertEqual(
            exampleLibraryItems(for: .adaptive).map(\.id),
            [IOS_HLS_PLAYLIST_ITEM_ID, IOS_DASH_PLAYLIST_ITEM_ID]
        )
        XCTAssertEqual(exampleLibraryItems(for: .live).map(\.id), [IOS_LIVE_DVR_PLAYLIST_ITEM_ID])
        XCTAssertTrue(exampleLibraryItems(for: .dolby).isEmpty)
        XCTAssertNil(exampleLibraryItem(playlistItemId: "unknown-item"))
    }

    func testDolbyCollectionCountsAcceptancePresets() {
        XCTAssertTrue(ExampleLibraryCollection.dolby.isDolbyAcceptance)
        XCTAssertEqual(exampleLibraryItemCount(.dolby, dolbyPresets: []), 0)
        XCTAssertEqual(exampleLibraryItemCount(.adaptive, dolbyPresets: []), 2)
    }

    func testLibraryItemCountUsesPluralRules() throws {
        // Hosted unit tests run inside the demo app, so its .lproj folders live in the main bundle.
        let bundle = Bundle.main
        func format(_ localization: String, _ count: Int) throws -> String {
            let path = try XCTUnwrap(bundle.path(forResource: localization, ofType: "lproj"), localization)
            let localized = try XCTUnwrap(Bundle(path: path))
            let pattern = localized.localizedString(
                forKey: "example.library.item_count",
                value: nil,
                table: "Localizable"
            )
            return String(format: pattern, locale: Locale(identifier: localization), count)
        }
        XCTAssertEqual(try format("en", 1), "1 item")
        XCTAssertEqual(try format("en", 0), "0 items")
        XCTAssertEqual(try format("en", 3), "3 items")
        XCTAssertEqual(try format("zh-Hans", 1), "1 项")
        XCTAssertEqual(try format("zh-Hans", 3), "3 项")
        XCTAssertFalse(ExampleI18n.libraryItemCount(1).contains("example.library.item_count"))
    }

    func testOpenMediaQuickSamplesCoverHlsDashAndLive() {
        XCTAssertEqual(
            exampleOpenMediaQuickSamples().map(\.id),
            [IOS_HLS_PLAYLIST_ITEM_ID, IOS_DASH_PLAYLIST_ITEM_ID, IOS_LIVE_DVR_PLAYLIST_ITEM_ID]
        )
    }
}
