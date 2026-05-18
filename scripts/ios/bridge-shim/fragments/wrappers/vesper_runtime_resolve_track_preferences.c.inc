{
  if (track_preferences == NULL || out_preferences == NULL) {
    return false;
  }

  PlayerFfiTrackPreferences ffi_track_preferences = {
      .preferred_audio_language = track_preferences->preferred_audio_language,
      .preferred_subtitle_language = track_preferences->preferred_subtitle_language,
      .select_subtitles_by_default = track_preferences->select_subtitles_by_default,
      .select_undetermined_subtitle_language =
          track_preferences->select_undetermined_subtitle_language,
      .audio_selection =
          {
              .mode =
                  (PlayerFfiTrackSelectionMode)track_preferences->audio_selection.mode_ordinal,
              .track_id = (char *)track_preferences->audio_selection.track_id,
          },
      .subtitle_selection =
          {
              .mode =
                  (PlayerFfiTrackSelectionMode)track_preferences->subtitle_selection.mode_ordinal,
              .track_id = (char *)track_preferences->subtitle_selection.track_id,
          },
      .abr_policy =
          {
              .mode = (PlayerFfiAbrMode)track_preferences->abr_policy.mode_ordinal,
              .track_id = (char *)track_preferences->abr_policy.track_id,
              .has_max_bit_rate = track_preferences->abr_policy.has_max_bit_rate,
              .max_bit_rate = non_negative_u64(track_preferences->abr_policy.max_bit_rate),
              .has_max_width = track_preferences->abr_policy.has_max_width,
              .max_width = non_negative_u32(track_preferences->abr_policy.max_width),
              .has_max_height = track_preferences->abr_policy.has_max_height,
              .max_height = non_negative_u32(track_preferences->abr_policy.max_height),
          },
  };
  PlayerFfiTrackPreferences ffi_resolved_preferences;
  PlayerFfiError ffi_error;
  memset(&ffi_resolved_preferences, 0, sizeof(ffi_resolved_preferences));
  memset(&ffi_error, 0, sizeof(ffi_error));

  PlayerFfiCallStatus status = player_ffi_resolve_track_preferences(
      &ffi_track_preferences,
      &ffi_resolved_preferences,
      &ffi_error);
  if (status != PlayerFfiCallStatusOk) {
    player_ffi_error_free(&ffi_error);
    return false;
  }

  out_preferences->preferred_audio_language =
      duplicate_string(ffi_resolved_preferences.preferred_audio_language);
  out_preferences->preferred_subtitle_language =
      duplicate_string(ffi_resolved_preferences.preferred_subtitle_language);
  out_preferences->select_subtitles_by_default =
      ffi_resolved_preferences.select_subtitles_by_default;
  out_preferences->select_undetermined_subtitle_language =
      ffi_resolved_preferences.select_undetermined_subtitle_language;
  out_preferences->audio_selection.mode_ordinal = ffi_resolved_preferences.audio_selection.mode;
  out_preferences->audio_selection.track_id =
      duplicate_string(ffi_resolved_preferences.audio_selection.track_id);
  out_preferences->subtitle_selection.mode_ordinal =
      ffi_resolved_preferences.subtitle_selection.mode;
  out_preferences->subtitle_selection.track_id =
      duplicate_string(ffi_resolved_preferences.subtitle_selection.track_id);
  out_preferences->abr_policy.mode_ordinal = ffi_resolved_preferences.abr_policy.mode;
  out_preferences->abr_policy.track_id =
      duplicate_string(ffi_resolved_preferences.abr_policy.track_id);
  out_preferences->abr_policy.has_max_bit_rate =
      ffi_resolved_preferences.abr_policy.has_max_bit_rate;
  out_preferences->abr_policy.max_bit_rate =
      (int64_t)ffi_resolved_preferences.abr_policy.max_bit_rate;
  out_preferences->abr_policy.has_max_width =
      ffi_resolved_preferences.abr_policy.has_max_width;
  out_preferences->abr_policy.max_width =
      (int32_t)ffi_resolved_preferences.abr_policy.max_width;
  out_preferences->abr_policy.has_max_height =
      ffi_resolved_preferences.abr_policy.has_max_height;
  out_preferences->abr_policy.max_height =
      (int32_t)ffi_resolved_preferences.abr_policy.max_height;

  player_ffi_track_preferences_free(&ffi_resolved_preferences);
  return true;
}
