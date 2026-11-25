import 'package:material_ui/material_ui.dart';

import 'example_host_navigation.dart';
import 'example_host_strings.dart';

extension ExampleHostTabText on ExampleHostTab {
  ExampleHostText get textKey {
    return switch (this) {
      ExampleHostTab.play => ExampleHostText.tabPlay,
      ExampleHostTab.library => ExampleHostText.tabLibrary,
      ExampleHostTab.downloads => ExampleHostText.tabDownloads,
      ExampleHostTab.lab => ExampleHostText.tabLab,
    };
  }

  IconData get icon {
    return switch (this) {
      ExampleHostTab.play => Icons.play_circle_outline_rounded,
      ExampleHostTab.library => Icons.video_library_outlined,
      ExampleHostTab.downloads => Icons.download_outlined,
      ExampleHostTab.lab => Icons.science_outlined,
    };
  }

  IconData get selectedIcon {
    return switch (this) {
      ExampleHostTab.play => Icons.play_circle_rounded,
      ExampleHostTab.library => Icons.video_library_rounded,
      ExampleHostTab.downloads => Icons.download_rounded,
      ExampleHostTab.lab => Icons.science_rounded,
    };
  }
}

/// Scaffold for the four host tabs: top bar with settings, bottom navigation
/// on phones, a navigation rail at [exampleNavigationRailMinWidth] and above,
/// and the open-media FAB on the play tab.
///
/// System back is routed through [onSystemBack] unless the model says the app
/// should exit. Modal sheets are routes above this widget, so the navigator
/// closes them first.
class ExampleHostShell extends StatelessWidget {
  const ExampleHostShell({
    super.key,
    required this.navigation,
    required this.isFullscreen,
    required this.title,
    required this.body,
    required this.fabExtended,
    required this.onSelectTab,
    required this.onSystemBack,
    required this.onOpenSettings,
    required this.onOpenMedia,
    this.onTitleBack,
    this.fabKey,
    this.onFabSize,
  });

  /// Receives the measured FAB size so scrolling bodies can reserve space.
  final ValueChanged<Size>? onFabSize;

  final ExampleHostNavigationState navigation;
  final bool isFullscreen;
  final String title;
  final Widget body;
  final bool fabExtended;
  final ValueChanged<ExampleHostTab> onSelectTab;
  final ValueChanged<ExampleBackAction> onSystemBack;
  final VoidCallback onOpenSettings;
  final VoidCallback onOpenMedia;
  final VoidCallback? onTitleBack;
  final Key? fabKey;

  @override
  Widget build(BuildContext context) {
    final strings = ExampleHostStrings.of(context);
    final backAction = navigation.backAction(autoFullscreen: isFullscreen);
    return PopScope<Object?>(
      canPop: backAction == ExampleBackAction.exitApp,
      onPopInvokedWithResult: (didPop, _) {
        if (!didPop) {
          onSystemBack(backAction);
        }
      },
      child: isFullscreen
          ? Scaffold(backgroundColor: Colors.black, body: body)
          : LayoutBuilder(
              builder: (context, constraints) {
                final useRail = exampleUsesNavigationRail(constraints.maxWidth);
                return useRail
                    ? _buildRailScaffold(context, strings)
                    : _buildBarScaffold(context, strings);
              },
            ),
    );
  }

  PreferredSizeWidget _buildTopBar(ExampleHostStrings strings) {
    return AppBar(
      leading: onTitleBack == null
          ? null
          : IconButton(
              key: const ValueKey<String>('shell-title-back'),
              tooltip: strings[ExampleHostText.back],
              icon: const BackButtonIcon(),
              onPressed: onTitleBack,
            ),
      automaticallyImplyLeading: false,
      title: Text(title, maxLines: 1, overflow: TextOverflow.ellipsis),
      actions: <Widget>[
        IconButton(
          key: const ValueKey<String>('shell-settings'),
          tooltip: strings[ExampleHostText.settings],
          icon: const Icon(Icons.settings_outlined),
          onPressed: onOpenSettings,
        ),
        const SizedBox(width: 4),
      ],
    );
  }

  bool get _showFab => navigation.tab == ExampleHostTab.play;

  Widget _buildBarScaffold(BuildContext context, ExampleHostStrings strings) {
    return Scaffold(
      appBar: _buildTopBar(strings),
      body: body,
      floatingActionButton: _showFab
          ? _ExampleSizeReporter(
              onSize: onFabSize,
              child: FloatingActionButton.extended(
                key: fabKey ?? const ValueKey<String>('shell-open-media-fab'),
                heroTag: null,
                onPressed: onOpenMedia,
                isExtended: fabExtended,
                tooltip: strings[ExampleHostText.openMedia],
                icon: const Icon(Icons.add_rounded),
                label: Text(strings[ExampleHostText.openMedia]),
              ),
            )
          : null,
      bottomNavigationBar: NavigationBar(
        key: const ValueKey<String>('shell-navigation-bar'),
        selectedIndex: navigation.tab.index,
        onDestinationSelected: (index) =>
            onSelectTab(ExampleHostTab.values[index]),
        destinations: <Widget>[
          for (final tab in ExampleHostTab.values)
            NavigationDestination(
              icon: Icon(tab.icon),
              selectedIcon: Icon(tab.selectedIcon),
              label: strings[tab.textKey],
            ),
        ],
      ),
    );
  }

  Widget _buildRailScaffold(BuildContext context, ExampleHostStrings strings) {
    return Scaffold(
      body: Row(
        children: <Widget>[
          SafeArea(
            right: false,
            child: NavigationRail(
              key: const ValueKey<String>('shell-navigation-rail'),
              selectedIndex: navigation.tab.index,
              labelType: NavigationRailLabelType.all,
              groupAlignment: -1,
              onDestinationSelected: (index) =>
                  onSelectTab(ExampleHostTab.values[index]),
              leading: Padding(
                padding: const EdgeInsets.only(top: 8, bottom: 16),
                child: FloatingActionButton(
                  key: fabKey ?? const ValueKey<String>('shell-open-media-fab'),
                  heroTag: null,
                  elevation: 1,
                  tooltip: strings[ExampleHostText.openMedia],
                  onPressed: onOpenMedia,
                  child: const Icon(Icons.add_rounded),
                ),
              ),
              destinations: <NavigationRailDestination>[
                for (final tab in ExampleHostTab.values)
                  NavigationRailDestination(
                    icon: Icon(tab.icon),
                    selectedIcon: Icon(tab.selectedIcon),
                    label: Text(strings[tab.textKey]),
                  ),
              ],
            ),
          ),
          Expanded(
            child: Scaffold(appBar: _buildTopBar(strings), body: body),
          ),
        ],
      ),
    );
  }
}

/// Reports the laid-out size of [child] after each frame it changes.
class _ExampleSizeReporter extends StatefulWidget {
  const _ExampleSizeReporter({required this.child, this.onSize});

  final Widget child;
  final ValueChanged<Size>? onSize;

  @override
  State<_ExampleSizeReporter> createState() => _ExampleSizeReporterState();
}

class _ExampleSizeReporterState extends State<_ExampleSizeReporter> {
  Size? _lastSize;

  void _report() {
    if (!mounted) {
      return;
    }
    final size = context.size;
    if (size == null || size == _lastSize) {
      return;
    }
    _lastSize = size;
    widget.onSize?.call(size);
  }

  @override
  Widget build(BuildContext context) {
    WidgetsBinding.instance.addPostFrameCallback((_) => _report());
    return widget.child;
  }
}
