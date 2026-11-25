/// Pure Dart navigation model for the example host shell.
///
/// Widgets render from [ExampleHostNavigationState] and feed user intents back
/// through its transition methods, which keeps tab, collection, fullscreen, and
/// back-key behavior unit-testable without a widget tree.
library;

enum ExampleHostTab { play, library, downloads, lab }

enum ExampleLibraryCollectionId { vod, live, adaptive, dolby }

/// What the next system back gesture should do once no sheet is open.
///
/// Modal sheets are routes above the shell, so the navigator closes them
/// before the shell is asked; [ExampleBackAction] covers the remaining order.
enum ExampleBackAction { exitFullscreen, closeCollection, goToPlayTab, exitApp }

/// Width at which the shell switches from a bottom bar to a navigation rail.
const double exampleNavigationRailMinWidth = 600;

/// Landscape windows below this height enter fullscreen automatically.
const double exampleAutoFullscreenMaxHeight = 480;

/// Scroll offset beyond which the extended FAB collapses to its icon.
const double exampleFabCollapseOffset = 8;

final class ExampleHostNavigationState {
  const ExampleHostNavigationState({
    this.tab = ExampleHostTab.play,
    this.openCollection,
    this.manualFullscreen = false,
  });

  final ExampleHostTab tab;
  final ExampleLibraryCollectionId? openCollection;
  final bool manualFullscreen;

  /// Whether the library is showing a collection detail page.
  bool get isCollectionVisible =>
      tab == ExampleHostTab.library && openCollection != null;

  /// Fullscreen is either requested explicitly or forced by a short
  /// landscape window, and only ever applies to the play tab.
  bool isFullscreen({required bool autoFullscreen}) {
    return tab == ExampleHostTab.play && (manualFullscreen || autoFullscreen);
  }

  ExampleHostNavigationState selectTab(ExampleHostTab next) {
    if (next == tab && next == ExampleHostTab.library) {
      // Re-selecting the active library tab returns to its root.
      return _copy(clearCollection: true);
    }
    return _copy(tab: next);
  }

  ExampleHostNavigationState showCollection(ExampleLibraryCollectionId id) {
    return _copy(tab: ExampleHostTab.library, openCollection: id);
  }

  ExampleHostNavigationState closeCollection() => _copy(clearCollection: true);

  ExampleHostNavigationState enterFullscreen() =>
      _copy(tab: ExampleHostTab.play, manualFullscreen: true);

  ExampleHostNavigationState exitFullscreen() => _copy(manualFullscreen: false);

  /// Playing an item from anywhere returns the user to the play tab.
  ExampleHostNavigationState showPlayer() => _copy(tab: ExampleHostTab.play);

  ExampleBackAction backAction({required bool autoFullscreen}) {
    if (isFullscreen(autoFullscreen: autoFullscreen)) {
      return ExampleBackAction.exitFullscreen;
    }
    if (isCollectionVisible) {
      return ExampleBackAction.closeCollection;
    }
    if (tab != ExampleHostTab.play) {
      return ExampleBackAction.goToPlayTab;
    }
    return ExampleBackAction.exitApp;
  }

  ExampleHostNavigationState applyBack(ExampleBackAction action) {
    return switch (action) {
      ExampleBackAction.exitFullscreen => exitFullscreen(),
      ExampleBackAction.closeCollection => closeCollection(),
      ExampleBackAction.goToPlayTab => showPlayer(),
      ExampleBackAction.exitApp => this,
    };
  }

  ExampleHostNavigationState _copy({
    ExampleHostTab? tab,
    ExampleLibraryCollectionId? openCollection,
    bool clearCollection = false,
    bool? manualFullscreen,
  }) {
    return ExampleHostNavigationState(
      tab: tab ?? this.tab,
      openCollection: clearCollection
          ? null
          : (openCollection ?? this.openCollection),
      manualFullscreen: manualFullscreen ?? this.manualFullscreen,
    );
  }

  @override
  bool operator ==(Object other) {
    return other is ExampleHostNavigationState &&
        other.tab == tab &&
        other.openCollection == openCollection &&
        other.manualFullscreen == manualFullscreen;
  }

  @override
  int get hashCode => Object.hash(tab, openCollection, manualFullscreen);

  @override
  String toString() =>
      'ExampleHostNavigationState($tab, $openCollection, '
      'manualFullscreen: $manualFullscreen)';
}

bool exampleShouldAutoFullscreen({
  required bool isLandscape,
  required double windowHeight,
}) {
  return isLandscape && windowHeight < exampleAutoFullscreenMaxHeight;
}

bool exampleUsesNavigationRail(double windowWidth) {
  return windowWidth >= exampleNavigationRailMinWidth;
}

bool exampleFabExtended(double scrollOffset) {
  return scrollOffset <= exampleFabCollapseOffset;
}

/// Bottom padding a scrolling list needs so its last item clears the FAB.
///
/// [fabHeight] is the measured FAB height, [fabMargin] the Scaffold's float
/// margin, and [bottomInset] any padding the Scaffold leaves to its body.
double exampleFabAwareBottomPadding({
  required double fabHeight,
  double fabMargin = 16,
  double gap = 16,
  double bottomInset = 0,
}) {
  final safeFab = fabHeight.isFinite && fabHeight > 0 ? fabHeight : 0.0;
  final safeInset = bottomInset.isFinite && bottomInset > 0 ? bottomInset : 0.0;
  if (safeFab == 0) {
    return gap + safeInset;
  }
  return safeFab + fabMargin + gap + safeInset;
}
