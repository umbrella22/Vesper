part of 'example_player_sections.dart';

class ExampleSystemPlaybackSection extends StatelessWidget {
  const ExampleSystemPlaybackSection({
    super.key,
    required this.palette,
    required this.controller,
    required this.permissionStatus,
    required this.onRequestPermission,
    required this.onRefreshExternalRoutes,
    required this.onExternalRoutePickerResult,
    required this.externalRoutes,
    required this.onExternalRouteSelected,
    required this.pictureInPictureStatus,
    required this.pictureInPictureEnabled,
    required this.onPictureInPictureEnabledChanged,
    required this.onRequestPictureInPicture,
    this.pictureInPictureAvailability,
    this.externalPlaybackMessage,
  });

  final ExampleHostPalette palette;
  final VesperPlayerController controller;
  final VesperSystemPlaybackPermissionStatus permissionStatus;
  final VoidCallback onRequestPermission;
  final VoidCallback onRefreshExternalRoutes;
  final ValueChanged<VesperExternalPlaybackResult> onExternalRoutePickerResult;
  final List<VesperExternalPlaybackRoute> externalRoutes;
  final ValueChanged<VesperExternalPlaybackRoute> onExternalRouteSelected;
  final VesperPictureInPictureAvailability? pictureInPictureAvailability;
  final VesperPictureInPictureStatus pictureInPictureStatus;
  final bool pictureInPictureEnabled;
  final ValueChanged<bool> onPictureInPictureEnabledChanged;
  final VoidCallback onRequestPictureInPicture;
  final String? externalPlaybackMessage;

  @override
  Widget build(BuildContext context) {
    final strings = ExampleHostStrings.of(context);
    return ExampleSectionShell(
      palette: palette,
      title: strings[ExampleHostText.labSystemPlayback],
      subtitle: strings[ExampleHostText.systemPlaybackSubtitle],
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: <Widget>[
          Wrap(
            spacing: 12,
            runSpacing: 12,
            crossAxisAlignment: WrapCrossAlignment.center,
            children: <Widget>[
              ConstrainedBox(
                constraints: const BoxConstraints(minWidth: 220),
                child: Material(
                  type: MaterialType.transparency,
                  child: SwitchListTile.adaptive(
                    contentPadding: EdgeInsets.zero,
                    dense: true,
                    value: pictureInPictureEnabled,
                    onChanged: onPictureInPictureEnabledChanged,
                    title: Text(
                      strings[ExampleHostText.pipEnable],
                      style: Theme.of(context).textTheme.bodyMedium?.copyWith(
                        color: palette.title,
                        fontWeight: FontWeight.w600,
                      ),
                    ),
                  ),
                ),
              ),
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
                child: VesperExternalRouteButton(
                  onResult: onExternalRoutePickerResult,
                ),
              ),
              OutlinedButton(
                onPressed: onRequestPermission,
                child: Text(
                  strings.format(
                    ExampleHostText.notificationPermission,
                    <String, Object?>{'status': permissionStatus.name},
                  ),
                ),
              ),
              OutlinedButton.icon(
                onPressed: onRefreshExternalRoutes,
                icon: const Icon(Icons.refresh, size: 18),
                label: Text(strings[ExampleHostText.rescanDlna]),
              ),
              OutlinedButton.icon(
                onPressed: _canRequestPictureInPicture
                    ? onRequestPictureInPicture
                    : null,
                icon: const Icon(
                  Icons.picture_in_picture_alt_rounded,
                  size: 18,
                ),
                label: Text(_pictureInPictureLabel(strings)),
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

  bool get _canRequestPictureInPicture {
    final availability = pictureInPictureAvailability;
    return pictureInPictureEnabled &&
        availability?.isAvailable == true &&
        pictureInPictureStatus != VesperPictureInPictureStatus.entering &&
        pictureInPictureStatus != VesperPictureInPictureStatus.active &&
        pictureInPictureStatus != VesperPictureInPictureStatus.exiting;
  }

  String _pictureInPictureLabel(ExampleHostStrings strings) {
    if (pictureInPictureStatus == VesperPictureInPictureStatus.active) {
      return strings[ExampleHostText.pipActive];
    }
    if (pictureInPictureStatus == VesperPictureInPictureStatus.entering) {
      return strings[ExampleHostText.pipEntering];
    }
    if (!pictureInPictureEnabled) {
      return strings[ExampleHostText.pipOff];
    }
    if (pictureInPictureAvailability?.isAvailable == false) {
      return strings[ExampleHostText.pipUnavailable];
    }
    return strings[ExampleHostText.pipRequest];
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
