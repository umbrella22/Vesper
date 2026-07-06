part of 'example_player_sections.dart';

class ExampleQueuePanel extends StatelessWidget {
  const ExampleQueuePanel({
    super.key,
    required this.palette,
    required this.playlistItems,
    required this.onSelectItem,
    required this.onManageQueue,
  });

  final ExampleHostPalette palette;
  final List<ExamplePlaylistItemViewData> playlistItems;
  final ValueChanged<String> onSelectItem;
  final VoidCallback onManageQueue;

  List<ExamplePlaylistItemViewData> get _compactItems {
    return compactQueueItems(playlistItems);
  }

  @override
  Widget build(BuildContext context) {
    return ExampleSectionShell(
      palette: palette,
      title: '待播放队列',
      subtitle: '这里只显示明确准备连续播放的媒体源。Dolby 预设默认留在诊断页，除非手动加入。',
      child: playlistItems.isEmpty
          ? Text(
              '队列里还没有媒体源。',
              style: Theme.of(
                context,
              ).textTheme.bodySmall?.copyWith(color: palette.body),
            )
          : Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: <Widget>[
                for (final item in _compactItems)
                  Padding(
                    key: ValueKey<String>(item.itemId),
                    padding: const EdgeInsets.only(bottom: 10),
                    child: _ExamplePlaylistRow(
                      item: item,
                      palette: palette,
                      onTap: () => onSelectItem(item.itemId),
                    ),
                  ),
                OutlinedButton.icon(
                  onPressed: onManageQueue,
                  icon: const Icon(Icons.list_rounded, size: 18),
                  label: const Text('管理队列'),
                ),
              ],
            ),
    );
  }
}

class ExampleQueueSheet extends StatelessWidget {
  const ExampleQueueSheet({
    super.key,
    required this.palette,
    required this.playlistItems,
    required this.onSelectItem,
  });

  final ExampleHostPalette palette;
  final List<ExamplePlaylistItemViewData> playlistItems;
  final ValueChanged<String> onSelectItem;

  @override
  Widget build(BuildContext context) {
    return SafeArea(
      child: Padding(
        padding: const EdgeInsets.fromLTRB(18, 14, 18, 24),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: <Widget>[
            Text(
              '管理队列',
              style: Theme.of(context).textTheme.titleMedium?.copyWith(
                color: palette.title,
                fontWeight: FontWeight.w700,
              ),
            ),
            const SizedBox(height: 14),
            Expanded(
              child: playlistItems.isEmpty
                  ? Text(
                      '队列里还没有媒体源。',
                      style: Theme.of(
                        context,
                      ).textTheme.bodySmall?.copyWith(color: palette.body),
                    )
                  : ListView.builder(
                      itemCount: playlistItems.length,
                      itemBuilder: (context, index) {
                        final item = playlistItems[index];
                        return Padding(
                          key: ValueKey<String>(item.itemId),
                          padding: const EdgeInsets.only(bottom: 10),
                          child: _ExamplePlaylistRow(
                            item: item,
                            palette: palette,
                            onTap: () {
                              Navigator.of(context).pop();
                              onSelectItem(item.itemId);
                            },
                          ),
                        );
                      },
                    ),
            ),
          ],
        ),
      ),
    );
  }
}

List<ExamplePlaylistItemViewData> compactQueueItems(
  List<ExamplePlaylistItemViewData> items, {
  int maxVisibleItems = 5,
}) {
  if (items.length <= maxVisibleItems) {
    return items;
  }
  final activeIndex = items.indexWhere((item) => item.isActive);
  if (activeIndex < 0) {
    return items.take(maxVisibleItems).toList(growable: false);
  }
  var lowerBound = activeIndex - 2;
  if (lowerBound < 0) {
    lowerBound = 0;
  }
  var upperBound = lowerBound + maxVisibleItems;
  if (upperBound > items.length) {
    upperBound = items.length;
  }
  var start = upperBound - maxVisibleItems;
  if (start < 0) {
    start = 0;
  }
  return items.sublist(start, upperBound);
}

class ExampleRecentErrorSection extends StatelessWidget {
  const ExampleRecentErrorSection({
    super.key,
    required this.palette,
    required this.error,
  });

  final ExampleHostPalette palette;
  final VesperPlayerError error;

  @override
  Widget build(BuildContext context) {
    final message = _exampleRecentErrorMessage(error);
    final diagnostics = _exampleRecentErrorDiagnostics(error).take(4).toList();
    return ExampleSectionShell(
      palette: palette,
      title: '最近错误',
      subtitle: '平台层错误会同时进入 snapshot 和 event stream。',
      accent: const Color(0xFFC13C36),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: <Widget>[
          Text(
            message,
            style: const TextStyle(color: Color(0xFF7F231F), height: 1.45),
          ),
          if (diagnostics.isNotEmpty) ...<Widget>[
            const SizedBox(height: 8),
            for (final line in diagnostics)
              Padding(
                padding: const EdgeInsets.only(top: 3),
                child: Text(
                  line,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: const TextStyle(
                    color: Color(0xFF7F463E),
                    fontSize: 12,
                    height: 1.25,
                  ),
                ),
              ),
          ],
        ],
      ),
    );
  }
}

String _exampleRecentErrorMessage(VesperPlayerError error) {
  if (_isDolbyVisionP5CapabilityFailure(error)) {
    return '当前设备不支持这个 Dolby Vision P5 / Widevine 播放组合。';
  }
  if (_isWidevineNetworkExhausted(error)) {
    final attempts = error.details['maxAttempts']?.toString();
    return 'Widevine license 或 provisioning 请求失败，已重试 ${attempts?.isNotEmpty == true ? attempts : '3'} 次。';
  }
  if (_isFairPlayNetworkExhausted(error)) {
    final attempts = error.details['maxAttempts']?.toString();
    return 'FairPlay license 或 certificate 请求失败，已重试 ${attempts?.isNotEmpty == true ? attempts : '3'} 次。';
  }
  return error.message;
}

Iterable<String> _exampleRecentErrorDiagnostics(VesperPlayerError error) sync* {
  final details = error.details;
  final licenseHost = details['licenseUriHost']?.toString();
  if (licenseHost != null && licenseHost.isNotEmpty) {
    yield 'license host：$licenseHost';
  }
  final certificateHost = details['certificateUriHost']?.toString();
  if (certificateHost != null && certificateHost.isNotEmpty) {
    yield 'certificate host：$certificateHost';
  }
  final httpStatusCode = details['httpStatusCode']?.toString();
  if (httpStatusCode != null && httpStatusCode.isNotEmpty) {
    yield 'HTTP status：$httpStatusCode';
  }
  final errorCode = details['errorCodeName']?.toString();
  if (errorCode != null && errorCode.isNotEmpty) {
    yield '错误码：$errorCode';
  }
  final codec = details['codec']?.toString();
  if (codec != null && codec.isNotEmpty) {
    yield 'codec：$codec';
  }
  final decoderName = details['decoderName']?.toString();
  if (decoderName != null && decoderName.isNotEmpty) {
    yield 'decoder：$decoderName';
  }
}

bool _isWidevineNetworkExhausted(VesperPlayerError error) {
  return error.category == VesperPlayerErrorCategory.network &&
      error.details['keySystem']?.toString().toLowerCase() == 'widevine' &&
      error.details['attemptsExhausted']?.toString().toLowerCase() == 'true';
}

bool _isFairPlayNetworkExhausted(VesperPlayerError error) {
  return error.category == VesperPlayerErrorCategory.network &&
      error.details['keySystem']?.toString().toLowerCase() == 'fairplay' &&
      error.details['attemptsExhausted']?.toString().toLowerCase() == 'true';
}

bool _isDolbyVisionP5CapabilityFailure(VesperPlayerError error) {
  if (error.category != VesperPlayerErrorCategory.decode &&
      error.category != VesperPlayerErrorCategory.capability) {
    return false;
  }
  final evidence = error.hdrCapabilityEvidence;
  final profile =
      evidence?.hdrMetadata?.dolbyVisionProfile?.toString() ??
      error.details['dolbyVisionProfile']?.toString();
  final codec = error.details['codec']?.toString().toLowerCase() ?? '';
  final sampleMimeType =
      error.details['sampleMimeType']?.toString().toLowerCase() ?? '';
  final cause =
      error.details['capabilityFailureCause']?.toString().toLowerCase() ?? '';
  return profile == '5' ||
      codec.contains('.05') ||
      sampleMimeType == 'video/dolby-vision' && cause.contains('decoder');
}

class _ExamplePlaylistRow extends StatelessWidget {
  const _ExamplePlaylistRow({
    required this.item,
    required this.palette,
    required this.onTap,
  });

  final ExamplePlaylistItemViewData item;
  final ExampleHostPalette palette;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return TextButton(
      onPressed: onTap,
      style: TextButton.styleFrom(
        padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 12),
        backgroundColor: item.isActive
            ? palette.primaryAction
            : palette.fieldBackground,
        foregroundColor: item.isActive ? Colors.white : palette.title,
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(18),
          side: BorderSide(
            color: item.isActive ? Colors.transparent : palette.sectionStroke,
          ),
        ),
      ),
      child: SizedBox(
        width: double.infinity,
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: <Widget>[
            Text(
              item.label,
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
              style: theme.textTheme.bodyLarge?.copyWith(
                fontWeight: FontWeight.w600,
                color: item.isActive ? Colors.white : palette.title,
              ),
            ),
            const SizedBox(height: 4),
            Text(
              item.status,
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
              style: theme.textTheme.labelMedium?.copyWith(
                color: item.isActive
                    ? Colors.white.withValues(alpha: 0.88)
                    : palette.body,
              ),
            ),
          ],
        ),
      ),
    );
  }
}
