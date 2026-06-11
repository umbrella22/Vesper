part of 'example_player_sections.dart';

class ExamplePluginDiagnosticsSection extends StatelessWidget {
  const ExamplePluginDiagnosticsSection({
    super.key,
    required this.palette,
    required this.sourceNormalizerSetting,
    required this.sourceNormalizerPluginLibraryPaths,
    required this.frameProcessorPluginLibraryPaths,
    required this.pluginDiagnostics,
    required this.isCapturingHdrEvidence,
    required this.hdrEvidenceActiveSourceAvailable,
    required this.hdrEvidencePresets,
    required this.selectedHdrEvidencePreset,
    required this.onSourceNormalizerSettingChange,
    required this.onHdrEvidencePresetChange,
    required this.onCaptureHdrEvidence,
  });

  final ExampleHostPalette palette;
  final ExampleSourceNormalizerSetting sourceNormalizerSetting;
  final List<String> sourceNormalizerPluginLibraryPaths;
  final List<String> frameProcessorPluginLibraryPaths;
  final List<VesperPluginDiagnostic> pluginDiagnostics;
  final bool isCapturingHdrEvidence;
  final bool hdrEvidenceActiveSourceAvailable;
  final List<ExampleHdrEvidenceSamplePreset> hdrEvidencePresets;
  final ExampleHdrEvidenceSamplePreset selectedHdrEvidencePreset;
  final ValueChanged<ExampleSourceNormalizerSetting>
  onSourceNormalizerSettingChange;
  final ValueChanged<ExampleHdrEvidenceSamplePreset> onHdrEvidencePresetChange;
  final VoidCallback onCaptureHdrEvidence;

  List<VesperPluginDiagnostic> get sourceNormalizerDiagnostics {
    return pluginDiagnostics
        .where((diagnostic) {
          return diagnostic.pluginKind == 'source_normalizer' ||
              diagnostic.status.name.startsWith('sourceNormalizer') ||
              diagnostic.capability?.kind ==
                  VesperPluginCapabilityKind.sourceNormalizer;
        })
        .toList(growable: false);
  }

  List<VesperPluginDiagnostic> get frameProcessorDiagnostics {
    return pluginDiagnostics
        .where((diagnostic) {
          return diagnostic.pluginKind == 'frame_processor' ||
              diagnostic.status.name.startsWith('frameProcessor') ||
              diagnostic.capability?.kind ==
                  VesperPluginCapabilityKind.frameProcessor;
        })
        .toList(growable: false);
  }

  @override
  Widget build(BuildContext context) {
    return ExampleSectionShell(
      palette: palette,
      title: '插件诊断',
      subtitle:
          'SourceNormalizer 可从 diagnostics/preflight 切到 normalized playback 路线；FrameProcessor 仅记录 debug 能力诊断。',
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: <Widget>[
          SingleChildScrollView(
            scrollDirection: Axis.horizontal,
            child: Row(
              children: ExampleSourceNormalizerSetting.values
                  .map(
                    (setting) => Padding(
                      padding: const EdgeInsets.only(right: 10),
                      child: ChoiceChip(
                        label: Text(setting.title),
                        selected: setting == sourceNormalizerSetting,
                        onSelected: setting == sourceNormalizerSetting
                            ? null
                            : (_) => onSourceNormalizerSettingChange(setting),
                      ),
                    ),
                  )
                  .toList(growable: false),
            ),
          ),
          const SizedBox(height: 12),
          Text(
            sourceNormalizerSetting.subtitle,
            style: Theme.of(
              context,
            ).textTheme.bodyMedium?.copyWith(color: palette.body, height: 1.45),
          ),
          const SizedBox(height: 14),
          ExampleFactRow(
            label: 'source',
            value: _pluginPathLabel(sourceNormalizerPluginLibraryPaths),
          ),
          ExampleFactRow(
            label: 'frame',
            value: _pluginPathLabel(frameProcessorPluginLibraryPaths),
          ),
          const SizedBox(height: 14),
          PluginDiagnosticGroup(
            title: 'SourceNormalizer',
            emptyLabel: '暂无 SourceNormalizer 诊断。',
            diagnostics: sourceNormalizerDiagnostics,
            palette: palette,
          ),
          const SizedBox(height: 14),
          PluginDiagnosticGroup(
            title: 'FrameProcessor Debug',
            emptyLabel: '暂无 FrameProcessor debug 诊断。',
            diagnostics: frameProcessorDiagnostics,
            palette: palette,
          ),
          const SizedBox(height: 14),
          Text(
            'HDR evidence',
            style: Theme.of(context).textTheme.labelLarge?.copyWith(
              color: palette.title,
              fontWeight: FontWeight.w700,
            ),
          ),
          const SizedBox(height: 8),
          SingleChildScrollView(
            scrollDirection: Axis.horizontal,
            child: Row(
              children: hdrEvidencePresets
                  .map(
                    (preset) => Padding(
                      padding: const EdgeInsets.only(right: 10),
                      child: ChoiceChip(
                        label: Text(preset.label),
                        selected:
                            preset.sampleId ==
                            selectedHdrEvidencePreset.sampleId,
                        onSelected: isCapturingHdrEvidence
                            ? null
                            : (_) => onHdrEvidencePresetChange(preset),
                      ),
                    ),
                  )
                  .toList(growable: false),
            ),
          ),
          const SizedBox(height: 10),
          Text(
            selectedHdrEvidencePreset.sampleId == 'NETWORK-FAILURE-CONTROL'
                ? 'Network control uses a fixed local HTTPS failure URL and should not produce HDR capability evidence.'
                : hdrEvidenceActiveSourceAvailable
                ? 'This preset will use the current active source; confirm metadata before capture.'
                : 'Select a local file or remote URL before capturing this preset.',
            style: Theme.of(
              context,
            ).textTheme.bodySmall?.copyWith(color: palette.body, height: 1.45),
          ),
          const SizedBox(height: 10),
          OutlinedButton.icon(
            onPressed:
                isCapturingHdrEvidence ||
                    (!hdrEvidenceActiveSourceAvailable &&
                        selectedHdrEvidencePreset.sampleId !=
                            'NETWORK-FAILURE-CONTROL')
                ? null
                : onCaptureHdrEvidence,
            icon: isCapturingHdrEvidence
                ? const SizedBox.square(
                    dimension: 16,
                    child: CircularProgressIndicator(strokeWidth: 2),
                  )
                : const Icon(Icons.fact_check_rounded, size: 18),
            label: Text(
              isCapturingHdrEvidence ? '正在采集 HDR evidence' : '采集 HDR evidence',
            ),
          ),
        ],
      ),
    );
  }
}

class PluginDiagnosticGroup extends StatelessWidget {
  const PluginDiagnosticGroup({
    super.key,
    required this.title,
    required this.emptyLabel,
    required this.diagnostics,
    required this.palette,
  });

  final String title;
  final String emptyLabel;
  final List<VesperPluginDiagnostic> diagnostics;
  final ExampleHostPalette palette;

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: <Widget>[
        Text(
          title,
          style: Theme.of(context).textTheme.labelLarge?.copyWith(
            color: palette.title,
            fontWeight: FontWeight.w700,
          ),
        ),
        const SizedBox(height: 8),
        if (diagnostics.isEmpty)
          Text(
            emptyLabel,
            style: Theme.of(
              context,
            ).textTheme.bodySmall?.copyWith(color: palette.body),
          )
        else
          ...diagnostics.map(
            (diagnostic) => Padding(
              padding: const EdgeInsets.only(bottom: 8),
              child: PluginDiagnosticRow(
                diagnostic: diagnostic,
                palette: palette,
              ),
            ),
          ),
      ],
    );
  }
}

class PluginDiagnosticRow extends StatelessWidget {
  const PluginDiagnosticRow({
    super.key,
    required this.diagnostic,
    required this.palette,
  });

  final VesperPluginDiagnostic diagnostic;
  final ExampleHostPalette palette;

  @override
  Widget build(BuildContext context) {
    final profiles =
        diagnostic.capability?.sourceNormalizer?.supportedRuntimeProfiles ??
        const <String>[];
    final extra = diagnostic.extra;
    final outputRoute =
        extra['route']?.toString() ?? extra['outputRoute']?.toString() ?? '';
    final selectedProfile = extra['selectedProfile']?.toString() ?? '';
    final primaryResource = extra['primaryResource']?.toString() ?? '';
    final fallbackReason = extra['fallbackReason']?.toString() ?? '';
    final diskBytesUsed = extra['diskBytesUsed'];
    final cachePolicy = extra['cachePolicy'];
    final cacheLimit =
        extra['cacheQuota'] ??
        (cachePolicy is Map ? cachePolicy['sessionDiskSoftCapBytes'] : null);
    final title = <String>[
      diagnostic.pluginName ?? '',
      diagnostic.status.name,
    ].where((value) => value.isNotEmpty).join(' · ');

    return Container(
      width: double.infinity,
      padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 12),
      decoration: BoxDecoration(
        color: palette.fieldBackground,
        borderRadius: BorderRadius.circular(18),
        border: Border.all(color: palette.sectionStroke),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: <Widget>[
          Text(
            title.isEmpty ? '插件诊断' : title,
            maxLines: 1,
            overflow: TextOverflow.ellipsis,
            style: Theme.of(context).textTheme.bodyMedium?.copyWith(
              color: palette.title,
              fontWeight: FontWeight.w700,
            ),
          ),
          const SizedBox(height: 5),
          Text(
            'participation: ${diagnostic.participation.name}',
            style: Theme.of(
              context,
            ).textTheme.bodySmall?.copyWith(color: palette.body),
          ),
          if (outputRoute.isNotEmpty || selectedProfile.isNotEmpty) ...<Widget>[
            const SizedBox(height: 5),
            Text(
              'route: ${<String>[outputRoute, selectedProfile].where((value) => value.isNotEmpty).join(' · ')}',
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
              style: Theme.of(
                context,
              ).textTheme.bodySmall?.copyWith(color: palette.body),
            ),
          ],
          if (diskBytesUsed is num || cacheLimit is num) ...<Widget>[
            const SizedBox(height: 5),
            Text(
              'cache: ${formatBytes((diskBytesUsed as num?)?.toInt())} / ${formatBytes((cacheLimit as num?)?.toInt())}',
              style: Theme.of(
                context,
              ).textTheme.bodySmall?.copyWith(color: palette.body),
            ),
          ],
          if (profiles.isNotEmpty) ...<Widget>[
            const SizedBox(height: 5),
            Text(
              'profiles: ${profiles.join(', ')}',
              maxLines: 2,
              overflow: TextOverflow.ellipsis,
              style: Theme.of(
                context,
              ).textTheme.bodySmall?.copyWith(color: palette.body),
            ),
          ],
          if ((diagnostic.message ?? '').isNotEmpty) ...<Widget>[
            const SizedBox(height: 5),
            Text(
              diagnostic.message!,
              maxLines: 3,
              overflow: TextOverflow.ellipsis,
              style: Theme.of(
                context,
              ).textTheme.bodySmall?.copyWith(color: palette.body),
            ),
          ],
          if (fallbackReason.isNotEmpty) ...<Widget>[
            const SizedBox(height: 5),
            Text(
              fallbackReason,
              maxLines: 2,
              overflow: TextOverflow.ellipsis,
              style: Theme.of(
                context,
              ).textTheme.bodySmall?.copyWith(color: palette.body),
            ),
          ],
          if (primaryResource.isNotEmpty) ...<Widget>[
            const SizedBox(height: 5),
            Text(
              'resource: $primaryResource',
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
              style: Theme.of(
                context,
              ).textTheme.labelSmall?.copyWith(color: palette.body),
            ),
          ],
          if (diagnostic.path.isNotEmpty) ...<Widget>[
            const SizedBox(height: 5),
            Text(
              diagnostic.path,
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
              style: Theme.of(
                context,
              ).textTheme.labelSmall?.copyWith(color: palette.body),
            ),
          ],
        ],
      ),
    );
  }
}

String _pluginPathLabel(List<String> paths) {
  return paths.isEmpty ? '缺失' : paths.join(', ');
}
