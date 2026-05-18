{
  if (track_preferences == NULL) {
    return;
  }

  free(track_preferences->preferred_audio_language);
  free(track_preferences->preferred_subtitle_language);
  free((void *)track_preferences->audio_selection.track_id);
  free((void *)track_preferences->subtitle_selection.track_id);
  free((void *)track_preferences->abr_policy.track_id);
  memset(track_preferences, 0, sizeof(*track_preferences));
}
