import 'package:material_ui/material_ui.dart';

import 'example_host_strings.dart';
import 'example_library_catalog.dart';

/// Validates a stream URL typed into the open-media sheet.
///
/// Returns the error copy key, or `null` when the URL can be opened.
ExampleHostText? exampleValidateStreamUrl(
  String raw, {
  required bool dashSupported,
}) {
  final value = raw.trim();
  if (value.isEmpty) {
    return ExampleHostText.urlErrorEmpty;
  }
  final uri = Uri.tryParse(value);
  if (uri == null ||
      !(uri.isScheme('http') || uri.isScheme('https')) ||
      uri.host.isEmpty) {
    return ExampleHostText.urlErrorInvalid;
  }
  if (!dashSupported && uri.path.toLowerCase().endsWith('.mpd')) {
    return ExampleHostText.urlErrorDashUnsupported;
  }
  return null;
}

/// Result of an open-media action: `null` means it opened, otherwise the
/// message to show while keeping the sheet open.
typedef ExampleOpenMediaAction = Future<String?> Function();

class ExampleOpenMediaSheet extends StatefulWidget {
  const ExampleOpenMediaSheet({
    super.key,
    required this.initialUrl,
    required this.localFilesEnabled,
    required this.dashEnabled,
    required this.onPickLocal,
    required this.onOpenUrl,
    required this.onOpenSample,
  });

  final String initialUrl;
  final bool localFilesEnabled;
  final bool dashEnabled;

  /// Returns `null` on success, an empty string when the user cancelled the
  /// picker (sheet stays open without an error), or an error message.
  final ExampleOpenMediaAction onPickLocal;
  final Future<String?> Function(String url) onOpenUrl;
  final Future<String?> Function(ExampleLibraryItem item) onOpenSample;

  @override
  State<ExampleOpenMediaSheet> createState() => _ExampleOpenMediaSheetState();
}

class _ExampleOpenMediaSheetState extends State<ExampleOpenMediaSheet> {
  late final TextEditingController _urlController = TextEditingController(
    text: widget.initialUrl,
  );
  String? _error;
  bool _busy = false;

  @override
  void dispose() {
    _urlController.dispose();
    super.dispose();
  }

  Future<void> _run(ExampleOpenMediaAction action) async {
    if (_busy) {
      return;
    }
    setState(() {
      _busy = true;
      _error = null;
    });
    final result = await action();
    if (!mounted) {
      return;
    }
    if (result == null) {
      Navigator.of(context).pop(true);
      return;
    }
    setState(() {
      _busy = false;
      _error = result.isEmpty ? null : result;
    });
  }

  void _submitUrl() {
    final strings = ExampleHostStrings.of(context);
    final errorKey = exampleValidateStreamUrl(
      _urlController.text,
      dashSupported: widget.dashEnabled,
    );
    if (errorKey != null) {
      setState(() => _error = strings[errorKey]);
      return;
    }
    _run(() => widget.onOpenUrl(_urlController.text.trim()));
  }

  @override
  Widget build(BuildContext context) {
    final strings = ExampleHostStrings.of(context);
    final theme = Theme.of(context);
    final bottomInset = MediaQuery.viewInsetsOf(context).bottom;
    return Padding(
      padding: EdgeInsets.only(bottom: bottomInset),
      child: SingleChildScrollView(
        padding: const EdgeInsets.fromLTRB(24, 0, 24, 28),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          mainAxisSize: MainAxisSize.min,
          children: <Widget>[
            Text(
              strings[ExampleHostText.openMedia],
              style: theme.textTheme.titleLarge,
            ),
            const SizedBox(height: 4),
            Text(
              strings[ExampleHostText.openMediaSubtitle],
              style: theme.textTheme.bodySmall?.copyWith(
                color: theme.colorScheme.onSurfaceVariant,
              ),
            ),
            const SizedBox(height: 12),
            _buildLocalTile(strings, theme),
            const SizedBox(height: 16),
            _buildUrlField(strings),
            const SizedBox(height: 20),
            Text(
              strings[ExampleHostText.openMediaQuickSamples],
              style: theme.textTheme.titleSmall,
            ),
            const SizedBox(height: 4),
            for (final sample in exampleOpenMediaSamples)
              _buildSampleTile(sample, strings, theme),
          ],
        ),
      ),
    );
  }

  Widget _buildLocalTile(ExampleHostStrings strings, ThemeData theme) {
    return ListTile(
      key: const ValueKey<String>('open-media-local'),
      contentPadding: EdgeInsets.zero,
      enabled: widget.localFilesEnabled && !_busy,
      leading: const Icon(Icons.video_library_outlined),
      title: Text(strings[ExampleHostText.openMediaPickLocal]),
      subtitle: Text(
        widget.localFilesEnabled
            ? strings[ExampleHostText.openMediaPickLocalSubtitle]
            : strings[ExampleHostText.openMediaPickLocalUnavailable],
      ),
      trailing: const Icon(Icons.chevron_right_rounded),
      onTap: () => _run(widget.onPickLocal),
    );
  }

  Widget _buildUrlField(ExampleHostStrings strings) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: <Widget>[
        TextField(
          key: const ValueKey<String>('open-media-url-field'),
          controller: _urlController,
          keyboardType: TextInputType.url,
          textInputAction: TextInputAction.go,
          maxLines: 1,
          enabled: !_busy,
          onSubmitted: (_) => _submitUrl(),
          onChanged: (_) {
            if (_error != null) {
              setState(() => _error = null);
            }
          },
          decoration: InputDecoration(
            labelText: strings[ExampleHostText.openMediaStreamUrl],
            helperText: _error == null
                ? strings[ExampleHostText.openMediaStreamUrlHint]
                : null,
            errorText: _error,
            errorMaxLines: 3,
          ),
        ),
        const SizedBox(height: 12),
        FilledButton(
          key: const ValueKey<String>('open-media-url-submit'),
          onPressed: _busy ? null : _submitUrl,
          child: Text(strings[ExampleHostText.openMediaOpenUrl]),
        ),
      ],
    );
  }

  Widget _buildSampleTile(
    ExampleLibraryItem sample,
    ExampleHostStrings strings,
    ThemeData theme,
  ) {
    final enabled = !_busy && (!sample.requiresDash || widget.dashEnabled);
    return ListTile(
      key: ValueKey<String>('open-media-sample-${sample.playlistItemId}'),
      contentPadding: EdgeInsets.zero,
      enabled: enabled,
      leading: const Icon(Icons.play_circle_outline_rounded),
      title: Text(strings[sample.titleKey]),
      subtitle: Text(
        sample.requiresDash && !widget.dashEnabled
            ? strings[ExampleHostText.dashUnavailable]
            : strings[sample.subtitleKey],
      ),
      onTap: () => _run(() => widget.onOpenSample(sample)),
    );
  }
}

/// Presents [ExampleOpenMediaSheet] as a modal bottom sheet with a drag
/// handle. Completes with `true` when media opened.
Future<bool> showExampleOpenMediaSheet(
  BuildContext context, {
  required ExampleOpenMediaSheet sheet,
}) async {
  final strings = ExampleHostStrings.of(context);
  final opened = await showModalBottomSheet<bool>(
    context: context,
    isScrollControlled: true,
    useSafeArea: true,
    showDragHandle: true,
    builder: (context) =>
        ExampleHostStringsScope(strings: strings, child: sheet),
  );
  return opened ?? false;
}
