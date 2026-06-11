part of 'example_player_sections.dart';

class ExampleSystemPlaybackSection extends StatelessWidget {
  const ExampleSystemPlaybackSection({
    super.key,
    required this.palette,
    required this.controller,
    required this.permissionStatus,
    required this.onRequestPermission,
    required this.onRefreshExternalRoutes,
    required this.externalRoutes,
    required this.onExternalRouteSelected,
    this.externalPlaybackMessage,
  });

  final ExampleHostPalette palette;
  final VesperPlayerController controller;
  final VesperSystemPlaybackPermissionStatus permissionStatus;
  final VoidCallback onRequestPermission;
  final VoidCallback onRefreshExternalRoutes;
  final List<VesperExternalPlaybackRoute> externalRoutes;
  final ValueChanged<VesperExternalPlaybackRoute> onExternalRouteSelected;
  final String? externalPlaybackMessage;

  @override
  Widget build(BuildContext context) {
    return ExampleSectionShell(
      palette: palette,
      title: '系统播放',
      subtitle: '后台音频、锁屏控制、AirPlay、Android Cast 和 DLNA 的宿主集成入口。',
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: <Widget>[
          Wrap(
            spacing: 12,
            runSpacing: 12,
            crossAxisAlignment: WrapCrossAlignment.center,
            children: <Widget>[
              _RouteButtonFrame(
                palette: palette,
                child: ui.VesperAirPlayRouteButton(
                  controller: controller,
                  tintColor: palette.title,
                  activeTintColor: palette.primaryAction,
                ),
              ),
              _RouteButtonFrame(
                palette: palette,
                child: const VesperExternalRouteButton(),
              ),
              OutlinedButton(
                onPressed: onRequestPermission,
                child: Text('通知权限：${permissionStatus.name}'),
              ),
              OutlinedButton.icon(
                onPressed: onRefreshExternalRoutes,
                icon: const Icon(Icons.refresh, size: 18),
                label: const Text('重新扫描 DLNA'),
              ),
            ],
          ),
          if (externalRoutes.isNotEmpty) ...<Widget>[
            const SizedBox(height: 12),
            Wrap(
              spacing: 8,
              runSpacing: 8,
              children: externalRoutes
                  .map(
                    (route) => OutlinedButton(
                      onPressed: () => onExternalRouteSelected(route),
                      child: Text(
                        '${route.kind.name}: ${route.name}',
                        overflow: TextOverflow.ellipsis,
                      ),
                    ),
                  )
                  .toList(growable: false),
            ),
          ],
          if (externalPlaybackMessage != null) ...<Widget>[
            const SizedBox(height: 12),
            Text(
              externalPlaybackMessage!,
              style: Theme.of(
                context,
              ).textTheme.bodySmall?.copyWith(color: palette.body),
            ),
          ],
        ],
      ),
    );
  }
}

class _RouteButtonFrame extends StatelessWidget {
  const _RouteButtonFrame({required this.palette, required this.child});

  final ExampleHostPalette palette;
  final Widget child;

  @override
  Widget build(BuildContext context) {
    return DecoratedBox(
      decoration: BoxDecoration(
        color: palette.sectionBackground,
        borderRadius: BorderRadius.circular(12),
        border: Border.all(color: palette.sectionStroke),
      ),
      child: Padding(padding: const EdgeInsets.all(4), child: child),
    );
  }
}
