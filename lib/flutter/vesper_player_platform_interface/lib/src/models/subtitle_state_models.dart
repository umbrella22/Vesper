part of '../models.dart';

/// Subtitle catalog lifecycle shared by the native host kits.
enum VesperSubtitleCatalogState {
  unavailable,
  loading,
  ready,
  failed,
  unknown;

  static VesperSubtitleCatalogState fromWire(String? raw) {
    for (final value in values) {
      if (value.name == raw) {
        return value;
      }
    }
    return raw == null
        ? VesperSubtitleCatalogState.unavailable
        : VesperSubtitleCatalogState.unknown;
  }
}

/// Subtitle selection transaction state shared by the native host kits.
///
/// `idle` describes a catalog with no active selection transaction. `applying`,
/// `confirmed`, and `failed` describe the latest request. The additional
/// compatibility values are retained for older experimental hosts; unknown
/// wire values remain available through [selectionStateRawValue].
enum VesperSubtitleSelectionState {
  idle,
  applying,
  confirmed,
  failed,
  unknown;

  static VesperSubtitleSelectionState fromWire(String? raw) {
    for (final value in values) {
      if (value.name == raw) {
        return value;
      }
    }
    return raw == null
        ? VesperSubtitleSelectionState.idle
        : VesperSubtitleSelectionState.unknown;
  }
}

/// Legacy subtitle lifecycle status.
///
/// This remains available for source compatibility. New code should use
/// [VesperSubtitleState.catalogState] and [VesperSubtitleState.selectionState].
enum VesperSubtitleStatus {
  unavailable,
  loading,
  ready,
  failed,
  unknown;

  static VesperSubtitleStatus fromWire(String? raw) {
    for (final value in values) {
      if (value.name == raw) {
        return value;
      }
    }
    return raw == null
        ? VesperSubtitleStatus.unavailable
        : VesperSubtitleStatus.unknown;
  }
}

/// Phase where a subtitle failure originated.
enum VesperSubtitleErrorPhase {
  manifest,
  resource,
  discovery,
  identity,
  selection,
  unknown;

  static VesperSubtitleErrorPhase fromWire(String? raw) {
    for (final value in values) {
      if (value.name == raw) {
        return value;
      }
    }
    return VesperSubtitleErrorPhase.unknown;
  }
}

/// Structured subtitle error carried alongside [VesperSubtitleState].
final class VesperSubtitleError {
  const VesperSubtitleError({
    required this.code,
    required this.phase,
    required this.retriable,
    required this.message,
    this.trackId,
    this.commandId,
    this.sourceEpoch,
    this.codeRawValue,
    this.phaseRawValue,
  });

  factory VesperSubtitleError.fromMap(Map<Object?, Object?> map) {
    final rawCode = map['code'];
    final rawPhase = map['phase'];
    return VesperSubtitleError(
      code: rawCode is String ? _subtitleErrorCodeFromWire(rawCode) : 'unknown',
      phase: VesperSubtitleErrorPhase.fromWire(
        rawPhase is String ? rawPhase : null,
      ),
      retriable: _decodeBool(map, 'retriable'),
      message: map['message'] as String? ?? '',
      trackId: map['trackId'] as String?,
      commandId: _decodeInt(map, 'commandId'),
      sourceEpoch: _decodeInt(map, 'sourceEpoch'),
      codeRawValue: rawCode is String ? rawCode : null,
      phaseRawValue: rawPhase is String ? rawPhase : null,
    );
  }

  final String code;
  final VesperSubtitleErrorPhase phase;
  final bool retriable;
  final String message;
  final String? trackId;
  final int? commandId;
  final int? sourceEpoch;
  final String? codeRawValue;
  final String? phaseRawValue;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'code': codeRawValue ?? code,
      'phase': phaseRawValue ?? phase.name,
      'retriable': retriable,
      'message': message,
      'trackId': trackId,
      if (commandId != null) 'commandId': commandId,
      if (sourceEpoch != null) 'sourceEpoch': sourceEpoch,
    };
  }
}

/// Maps a wire `code` string into a stable identifier while preserving the
/// original value through [VesperSubtitleError.codeRawValue].
String _subtitleErrorCodeFromWire(String raw) {
  final trimmed = raw.trim();
  return trimmed.isEmpty ? 'unknown' : trimmed;
}

/// Snapshot of subtitle catalog and selection lifecycle exposed by host kits.
///
/// The canonical wire fields are `catalogState`, `selectionState`,
/// `advertisedTrackCount`, `selectableTrackCount`, `catalogError`, and
/// `selectionError`. The legacy `status` / `error` fields remain readable and
/// are emitted as aliases so older host kits and Flutter clients continue to
/// interoperate.
final class VesperSubtitleState {
  const VesperSubtitleState({
    VesperSubtitleCatalogState? catalogState,
    VesperSubtitleSelectionState? selectionState,
    this.advertisedTrackCount = 0,
    this.selectableTrackCount = 0,
    VesperSubtitleError? catalogError,
    VesperSubtitleError? selectionError,
    VesperSubtitleStatus? status,
    VesperSubtitleError? error,
    String? statusRawValue,
    String? catalogStateRawValue,
    String? selectionStateRawValue,
  })  : _catalogState = catalogState,
        _selectionState = selectionState,
        _catalogError = catalogError,
        _selectionError = selectionError,
        _legacyStatus = catalogState == null ? status : null,
        _legacyError = catalogState == null &&
                selectionState == null &&
                catalogError == null &&
                selectionError == null
            ? error
            : null,
        _catalogStateRawValue = catalogStateRawValue ??
            (catalogState == null ? statusRawValue : null),
        _selectionStateRawValue = selectionStateRawValue;

  /// Empty / initial state used when the host payload omits the field.
  static const empty = VesperSubtitleState();

  /// Creates an unavailable catalog with an idle selection state.
  const VesperSubtitleState.unavailable() : this();

  /// Creates a catalog-loading state.
  factory VesperSubtitleState.loading({required int advertisedTrackCount}) {
    return VesperSubtitleState(
      catalogState: VesperSubtitleCatalogState.loading,
      selectionState: VesperSubtitleSelectionState.idle,
      advertisedTrackCount: advertisedTrackCount,
    );
  }

  /// Creates a ready catalog state with an idle selection transaction.
  factory VesperSubtitleState.ready({
    required int advertisedTrackCount,
    required int selectableTrackCount,
  }) {
    return VesperSubtitleState(
      catalogState: VesperSubtitleCatalogState.ready,
      selectionState: VesperSubtitleSelectionState.idle,
      advertisedTrackCount: advertisedTrackCount,
      selectableTrackCount: selectableTrackCount,
    );
  }

  /// Creates a structured catalog or selection failure state.
  factory VesperSubtitleState.failed({
    required int advertisedTrackCount,
    required String code,
    required VesperSubtitleErrorPhase phase,
    required String message,
    String? trackId,
    bool retriable = false,
    int selectableTrackCount = 0,
    int? commandId,
    int? sourceEpoch,
  }) {
    final error = VesperSubtitleError(
      code: code,
      phase: phase,
      retriable: retriable,
      message: message,
      trackId: trackId,
      commandId: commandId,
      sourceEpoch: sourceEpoch,
    );
    final isSelectionFailure = phase == VesperSubtitleErrorPhase.selection;
    return VesperSubtitleState(
      catalogState: isSelectionFailure
          ? VesperSubtitleCatalogState.ready
          : VesperSubtitleCatalogState.failed,
      selectionState: isSelectionFailure
          ? VesperSubtitleSelectionState.failed
          : VesperSubtitleSelectionState.idle,
      advertisedTrackCount: advertisedTrackCount,
      selectableTrackCount: selectableTrackCount,
      catalogError: isSelectionFailure ? null : error,
      selectionError: isSelectionFailure ? error : null,
    );
  }

  factory VesperSubtitleState.fromMap(Map<Object?, Object?> map) {
    final rawCatalogState = map['catalogState'];
    final rawSelectionState = map['selectionState'];
    final rawStatus = map['status'];
    final rawCatalogError = _subtitleErrorFromWire(map['catalogError']);
    final rawSelectionError = _subtitleErrorFromWire(map['selectionError']);
    final legacyError = _subtitleErrorFromWire(map['error']);
    final hasCatalogState = rawCatalogState is String;
    final hasSelectionState = rawSelectionState is String;
    final legacyStatus =
        rawStatus is String ? VesperSubtitleStatus.fromWire(rawStatus) : null;
    final advertisedTrackCount = _decodeInt(map, 'advertisedTrackCount') ?? 0;
    final selectableTrackCount = _decodeInt(map, 'selectableTrackCount') ?? 0;
    final hasCatalogError = map.containsKey('catalogError');
    final hasSelectionError = map.containsKey('selectionError');
    final legacyCatalogError = legacyError != null &&
            legacyError.phase != VesperSubtitleErrorPhase.selection
        ? legacyError
        : null;
    final legacySelectionError = legacyError != null &&
            legacyError.phase == VesperSubtitleErrorPhase.selection
        ? legacyError
        : null;
    return VesperSubtitleState(
      catalogState: hasCatalogState
          ? VesperSubtitleCatalogState.fromWire(rawCatalogState)
          : null,
      selectionState: hasSelectionState
          ? VesperSubtitleSelectionState.fromWire(rawSelectionState)
          : _selectionStateFromLegacy(
              legacyStatus,
              legacyError,
              selectableTrackCount,
            ),
      advertisedTrackCount: advertisedTrackCount,
      selectableTrackCount: selectableTrackCount,
      catalogError: hasCatalogError ? rawCatalogError : legacyCatalogError,
      selectionError: hasSelectionError || hasSelectionState
          ? rawSelectionError
          : legacySelectionError,
      status: hasCatalogState ? null : legacyStatus,
      statusRawValue: rawStatus is String ? rawStatus : null,
      catalogStateRawValue: hasCatalogState
          ? rawCatalogState
          : rawStatus is String
              ? rawStatus
              : null,
      selectionStateRawValue:
          rawSelectionState is String ? rawSelectionState : null,
    );
  }

  final VesperSubtitleCatalogState? _catalogState;
  final VesperSubtitleSelectionState? _selectionState;
  final int advertisedTrackCount;
  final int selectableTrackCount;
  final VesperSubtitleError? _catalogError;
  final VesperSubtitleError? _selectionError;
  final VesperSubtitleStatus? _legacyStatus;
  final VesperSubtitleError? _legacyError;

  final String? _catalogStateRawValue;
  final String? _selectionStateRawValue;

  VesperSubtitleCatalogState get catalogState =>
      _catalogState ?? _catalogStateFromLegacy(_legacyStatus);

  VesperSubtitleSelectionState get selectionState =>
      _selectionState ??
      _selectionStateFromLegacy(
        _legacyStatus,
        _legacyError,
        selectableTrackCount,
      );

  VesperSubtitleError? get catalogError =>
      _catalogError ??
      (_legacyError?.phase != VesperSubtitleErrorPhase.selection
          ? _legacyError
          : null);

  VesperSubtitleError? get selectionError =>
      _selectionError ??
      (_legacyError?.phase == VesperSubtitleErrorPhase.selection
          ? _legacyError
          : null);

  /// Legacy catalog status alias.
  @Deprecated('Use catalogState instead.')
  VesperSubtitleStatus get status => _statusFromCatalog(catalogState);

  /// Legacy error alias. Selection errors take precedence because they
  /// describe the most recent user-visible command failure.
  @Deprecated('Use catalogError or selectionError instead.')
  VesperSubtitleError? get error => selectionError ?? catalogError;

  /// Legacy raw status alias.
  @Deprecated('Use catalogStateRawValue instead.')
  String? get statusRawValue => _catalogStateRawValue;

  /// Raw catalog state retained when a newer host sends an unknown value.
  String? get catalogStateRawValue => _catalogStateRawValue;

  /// Raw selection state retained when a newer host sends an unknown value.
  String? get selectionStateRawValue => _selectionStateRawValue;

  Map<String, Object?> toMap() {
    final legacyStatusWire =
        _catalogStateRawValue ?? _statusFromCatalog(catalogState).name;
    return <String, Object?>{
      'catalogState': _catalogStateRawValue ?? catalogState.name,
      'selectionState': _selectionStateRawValue ?? selectionState.name,
      'advertisedTrackCount': advertisedTrackCount,
      'selectableTrackCount': selectableTrackCount,
      'catalogError': catalogError?.toMap(),
      'selectionError': selectionError?.toMap(),
      // Legacy aliases are intentionally kept on output for older hosts.
      'status': legacyStatusWire,
      'error': error?.toMap(),
    };
  }

  VesperSubtitleState copyWith({
    VesperSubtitleCatalogState? catalogState,
    VesperSubtitleSelectionState? selectionState,
    int? advertisedTrackCount,
    int? selectableTrackCount,
    Object? catalogError = _subtitleStateSentinel,
    Object? selectionError = _subtitleStateSentinel,
    VesperSubtitleStatus? status,
    Object? error = _subtitleStateSentinel,
    Object? statusRawValue = _subtitleStateSentinel,
    Object? catalogStateRawValue = _subtitleStateSentinel,
    Object? selectionStateRawValue = _subtitleStateSentinel,
  }) {
    final nextCatalogState = catalogState ??
        (status == null ? this.catalogState : _catalogStateFromLegacy(status));
    final legacyErrorProvided = !identical(error, _subtitleStateSentinel);
    final legacyErrorValue =
        legacyErrorProvided ? error as VesperSubtitleError? : null;
    final nextCatalogError = identical(catalogError, _subtitleStateSentinel)
        ? (legacyErrorProvided
            ? (legacyErrorValue == null ||
                    legacyErrorValue.phase != VesperSubtitleErrorPhase.selection
                ? legacyErrorValue
                : null)
            : this.catalogError)
        : catalogError as VesperSubtitleError?;
    final nextSelectionError = identical(selectionError, _subtitleStateSentinel)
        ? (legacyErrorProvided
            ? (legacyErrorValue != null &&
                    legacyErrorValue.phase == VesperSubtitleErrorPhase.selection
                ? legacyErrorValue
                : null)
            : this.selectionError)
        : selectionError as VesperSubtitleError?;
    final nextCatalogStateRawValue = !identical(
      catalogStateRawValue,
      _subtitleStateSentinel,
    )
        ? catalogStateRawValue as String?
        : !identical(statusRawValue, _subtitleStateSentinel)
            ? statusRawValue as String?
            : catalogState == null && status == null
                ? _catalogStateRawValue
                : null;
    return VesperSubtitleState(
      catalogState: nextCatalogState,
      selectionState: selectionState ?? this.selectionState,
      advertisedTrackCount: advertisedTrackCount ?? this.advertisedTrackCount,
      selectableTrackCount: selectableTrackCount ?? this.selectableTrackCount,
      catalogError: nextCatalogError,
      selectionError: nextSelectionError,
      catalogStateRawValue: nextCatalogStateRawValue,
      selectionStateRawValue: identical(
        selectionStateRawValue,
        _subtitleStateSentinel,
      )
          ? (selectionState == null ? _selectionStateRawValue : null)
          : selectionStateRawValue as String?,
    );
  }
}

VesperSubtitleError? _subtitleErrorFromWire(Object? raw) {
  final map = _rawMap(raw);
  return map == null ? null : VesperSubtitleError.fromMap(map);
}

VesperSubtitleCatalogState _catalogStateFromLegacy(
  VesperSubtitleStatus? status,
) {
  switch (status) {
    case VesperSubtitleStatus.loading:
      return VesperSubtitleCatalogState.loading;
    case VesperSubtitleStatus.ready:
      return VesperSubtitleCatalogState.ready;
    case VesperSubtitleStatus.failed:
      return VesperSubtitleCatalogState.failed;
    case VesperSubtitleStatus.unknown:
      return VesperSubtitleCatalogState.unknown;
    case VesperSubtitleStatus.unavailable:
    case null:
      return VesperSubtitleCatalogState.unavailable;
  }
}

VesperSubtitleSelectionState _selectionStateFromLegacy(
  VesperSubtitleStatus? status,
  VesperSubtitleError? error,
  int selectableTrackCount,
) {
  if (error?.phase == VesperSubtitleErrorPhase.selection ||
      status == VesperSubtitleStatus.failed &&
          error?.phase == VesperSubtitleErrorPhase.selection) {
    return VesperSubtitleSelectionState.failed;
  }
  if (selectableTrackCount > 0) {
    return VesperSubtitleSelectionState.idle;
  }
  return VesperSubtitleSelectionState.idle;
}

VesperSubtitleStatus _statusFromCatalog(VesperSubtitleCatalogState state) {
  switch (state) {
    case VesperSubtitleCatalogState.unavailable:
      return VesperSubtitleStatus.unavailable;
    case VesperSubtitleCatalogState.loading:
      return VesperSubtitleStatus.loading;
    case VesperSubtitleCatalogState.ready:
      return VesperSubtitleStatus.ready;
    case VesperSubtitleCatalogState.failed:
      return VesperSubtitleStatus.failed;
    case VesperSubtitleCatalogState.unknown:
      return VesperSubtitleStatus.unknown;
  }
}

const Object _subtitleStateSentinel = Object();
