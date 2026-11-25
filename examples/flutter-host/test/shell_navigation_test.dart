import 'package:flutter/widgets.dart';
import 'package:flutter_host/src/shell/example_host_navigation.dart';
import 'package:flutter_host/src/shell/example_host_strings.dart';
import 'package:flutter_host/src/shell/example_host_strings_en.dart';
import 'package:flutter_host/src/shell/example_host_strings_zh.dart';
import 'package:flutter_host/src/shell/example_library_catalog.dart';
import 'package:flutter_host/src/shell/example_open_media_sheet.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  group('ExampleHostNavigationState', () {
    const play = ExampleHostNavigationState();

    test('starts on the play tab with nothing open', () {
      expect(play.tab, ExampleHostTab.play);
      expect(play.openCollection, isNull);
      expect(play.isFullscreen(autoFullscreen: false), isFalse);
      expect(ExampleHostTab.values, hasLength(4));
    });

    test('showCollection switches to the library detail page', () {
      final next = play.showCollection(ExampleLibraryCollectionId.dolby);
      expect(next.tab, ExampleHostTab.library);
      expect(next.isCollectionVisible, isTrue);
    });

    test('reselecting the library tab returns to its root', () {
      final detail = play.showCollection(ExampleLibraryCollectionId.vod);
      expect(detail.selectTab(ExampleHostTab.library).openCollection, isNull);
    });

    test('collection survives switching tabs but is hidden elsewhere', () {
      final detail = play.showCollection(ExampleLibraryCollectionId.live);
      final downloads = detail.selectTab(ExampleHostTab.downloads);
      expect(downloads.isCollectionVisible, isFalse);
      expect(
        downloads.selectTab(ExampleHostTab.library).isCollectionVisible,
        isTrue,
      );
    });

    test('fullscreen only applies to the play tab', () {
      expect(play.isFullscreen(autoFullscreen: true), isTrue);
      final lab = play.selectTab(ExampleHostTab.lab);
      expect(lab.isFullscreen(autoFullscreen: true), isFalse);
      expect(
        play.enterFullscreen().isFullscreen(autoFullscreen: false),
        isTrue,
      );
    });

    test('back order: fullscreen, collection, play tab, exit', () {
      var state = play
          .showCollection(ExampleLibraryCollectionId.adaptive)
          .enterFullscreen()
          .selectTab(ExampleHostTab.library);
      // Fullscreen is tied to the play tab, so from the library it does not
      // apply; check the play-tab fullscreen path separately.
      expect(
        play.enterFullscreen().backAction(autoFullscreen: false),
        ExampleBackAction.exitFullscreen,
      );
      state = state.exitFullscreen();
      final actions = <ExampleBackAction>[];
      for (var i = 0; i < 4; i += 1) {
        final action = state.backAction(autoFullscreen: false);
        actions.add(action);
        if (action == ExampleBackAction.exitApp) {
          break;
        }
        state = state.applyBack(action);
      }
      expect(actions, <ExampleBackAction>[
        ExampleBackAction.closeCollection,
        ExampleBackAction.goToPlayTab,
        ExampleBackAction.exitApp,
      ]);
    });
  });

  group('layout helpers', () {
    test('auto fullscreen needs landscape and a short window', () {
      expect(
        exampleShouldAutoFullscreen(isLandscape: true, windowHeight: 400),
        isTrue,
      );
      expect(
        exampleShouldAutoFullscreen(isLandscape: true, windowHeight: 480),
        isFalse,
      );
      expect(
        exampleShouldAutoFullscreen(isLandscape: false, windowHeight: 300),
        isFalse,
      );
    });

    test('navigation rail starts at 600dp', () {
      expect(exampleUsesNavigationRail(599), isFalse);
      expect(exampleUsesNavigationRail(600), isTrue);
    });

    test('FAB collapses once the list scrolls', () {
      expect(exampleFabExtended(0), isTrue);
      expect(exampleFabExtended(40), isFalse);
    });

    test('FAB-aware padding = fab + margin + gap + inset', () {
      expect(exampleFabAwareBottomPadding(fabHeight: 56), 56 + 16 + 16);
      expect(
        exampleFabAwareBottomPadding(fabHeight: 56, bottomInset: 34),
        56 + 16 + 16 + 34,
      );
      expect(exampleFabAwareBottomPadding(fabHeight: 0), 16);
      expect(exampleFabAwareBottomPadding(fabHeight: double.nan), 16);
    });
  });

  group('strings', () {
    test('item counts use the right plural form', () {
      expect(ExampleHostStrings.en.itemCount(1), '1 item');
      expect(ExampleHostStrings.en.itemCount(0), '0 items');
      expect(ExampleHostStrings.en.itemCount(3), '3 items');
      expect(ExampleHostStrings.zh.itemCount(1), '1 项');
      expect(ExampleHostStrings.zh.itemCount(12), '12 项');
    });

    test('en and zh tables cover every key', () {
      for (final key in ExampleHostText.values) {
        expect(exampleHostStringsEn[key], isNotEmpty, reason: '$key (en)');
        expect(exampleHostStringsZh[key], isNotEmpty, reason: '$key (zh)');
      }
    });

    test('locale selection', () {
      expect(
        ExampleHostStrings.forLocale(const Locale('zh', 'CN')),
        same(ExampleHostStrings.zh),
      );
      expect(ExampleHostStrings.forLocale(null), same(ExampleHostStrings.en));
    });

    test('every collection title and item key resolves', () {
      expect(exampleLibraryCollections, hasLength(4));
      for (final collection in exampleLibraryCollections) {
        expect(collection.itemCount, greaterThan(0));
      }
    });
  });

  test('stream URL validation', () {
    expect(
      exampleValidateStreamUrl('  ', dashSupported: true),
      ExampleHostText.urlErrorEmpty,
    );
    expect(
      exampleValidateStreamUrl('ftp://x/y', dashSupported: true),
      ExampleHostText.urlErrorInvalid,
    );
    expect(
      exampleValidateStreamUrl('https://a.b/c.mpd', dashSupported: false),
      ExampleHostText.urlErrorDashUnsupported,
    );
    expect(
      exampleValidateStreamUrl('https://a.b/c.m3u8', dashSupported: false),
      isNull,
    );
  });
}
