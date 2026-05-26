use super::*;

fn free_video_info(video: &mut PlayerFfiVideoInfo) {
    free_c_string(&mut video.codec);
    *video = PlayerFfiVideoInfo::default();
}

pub(crate) fn free_audio_info(audio: &mut PlayerFfiAudioInfo) {
    free_c_string(&mut audio.codec);
    *audio = PlayerFfiAudioInfo::default();
}

pub(crate) fn free_track(track: &mut PlayerFfiTrack) {
    free_c_string(&mut track.id);
    free_c_string(&mut track.label);
    free_c_string(&mut track.language);
    free_c_string(&mut track.codec);
    *track = PlayerFfiTrack::default();
}

pub(crate) fn free_track_catalog(track_catalog: &mut PlayerFfiTrackCatalog) {
    if !track_catalog.tracks.is_null() {
        unsafe {
            let mut boxed = Box::from_raw(ptr::slice_from_raw_parts_mut(
                track_catalog.tracks,
                track_catalog.len,
            ));
            for track in boxed.iter_mut() {
                free_track(track);
            }
        }
    }
    *track_catalog = PlayerFfiTrackCatalog::default();
}

pub(crate) fn free_track_selection(track_selection: &mut PlayerFfiTrackSelection) {
    free_c_string(&mut track_selection.track_id);
    *track_selection = PlayerFfiTrackSelection::default();
}

pub(crate) fn free_abr_policy(abr_policy: &mut PlayerFfiAbrPolicy) {
    free_c_string(&mut abr_policy.track_id);
    *abr_policy = PlayerFfiAbrPolicy::default();
}

pub(crate) fn free_track_selection_snapshot(track_selection: &mut PlayerFfiTrackSelectionSnapshot) {
    free_track_selection(&mut track_selection.video);
    free_track_selection(&mut track_selection.audio);
    free_track_selection(&mut track_selection.subtitle);
    free_abr_policy(&mut track_selection.abr_policy);
    *track_selection = PlayerFfiTrackSelectionSnapshot::default();
}

pub(crate) fn free_track_preferences(track_preferences: &mut PlayerFfiTrackPreferences) {
    free_c_string(&mut track_preferences.preferred_audio_language);
    free_c_string(&mut track_preferences.preferred_subtitle_language);
    free_track_selection(&mut track_preferences.audio_selection);
    free_track_selection(&mut track_preferences.subtitle_selection);
    free_abr_policy(&mut track_preferences.abr_policy);
    *track_preferences = PlayerFfiTrackPreferences::default();
}

pub(crate) fn free_media_info(media_info: &mut PlayerFfiMediaInfo) {
    free_c_string(&mut media_info.source_uri);
    free_video_info(&mut media_info.best_video);
    free_audio_info(&mut media_info.best_audio);
    free_track_catalog(&mut media_info.track_catalog);
    free_track_selection_snapshot(&mut media_info.track_selection);
    *media_info = PlayerFfiMediaInfo::default();
}

pub(crate) fn free_audio_output(audio_output: &mut PlayerFfiAudioOutputInfo) {
    free_c_string(&mut audio_output.device_name);
    free_c_string(&mut audio_output.sample_format);
    *audio_output = PlayerFfiAudioOutputInfo::default();
}

pub(crate) fn free_plugin_codec_capability(codec: &mut PlayerFfiPluginCodecCapability) {
    free_c_string(&mut codec.media_kind);
    free_c_string(&mut codec.codec);
    *codec = PlayerFfiPluginCodecCapability::default();
}

pub(crate) fn free_plugin_decoder_capability(
    capability: &mut PlayerFfiPluginDecoderCapabilitySummary,
) {
    if !capability.codecs.is_null() {
        unsafe {
            let mut boxed = Box::from_raw(ptr::slice_from_raw_parts_mut(
                capability.codecs,
                capability.codecs_len,
            ));
            for codec in boxed.iter_mut() {
                free_plugin_codec_capability(codec);
            }
        }
    }
    free_c_string_array(&mut capability.legacy_codecs, capability.legacy_codecs_len);
    *capability = PlayerFfiPluginDecoderCapabilitySummary::default();
}

pub(crate) fn free_plugin_frame_processor_capability(
    capability: &mut PlayerFfiPluginFrameProcessorCapabilitySummary,
) {
    free_c_string_array(
        &mut capability.accepted_input_handle_kinds,
        capability.accepted_input_handle_kinds_len,
    );
    free_c_string_array(
        &mut capability.output_handle_kinds,
        capability.output_handle_kinds_len,
    );
    *capability = PlayerFfiPluginFrameProcessorCapabilitySummary::default();
}

pub(crate) fn free_plugin_source_normalizer_capability(
    capability: &mut PlayerFfiPluginSourceNormalizerCapabilitySummary,
) {
    free_c_string_array(
        &mut capability.supported_runtime_profiles,
        capability.supported_runtime_profiles_len,
    );
    free_c_string_array(
        &mut capability.supported_output_routes,
        capability.supported_output_routes_len,
    );
    free_c_string(&mut capability.max_level);
    free_c_string_array(&mut capability.media_kinds, capability.media_kinds_len);
    free_c_string_array(&mut capability.codecs, capability.codecs_len);
    free_c_string_array(
        &mut capability.bitstream_formats,
        capability.bitstream_formats_len,
    );
    free_c_string_array(&mut capability.content_types, capability.content_types_len);
    free_c_string_array(
        &mut capability.required_libraries,
        capability.required_libraries_len,
    );
    free_c_string_array(
        &mut capability.required_demuxers,
        capability.required_demuxers_len,
    );
    free_c_string_array(
        &mut capability.required_muxers,
        capability.required_muxers_len,
    );
    free_c_string_array(
        &mut capability.required_protocols,
        capability.required_protocols_len,
    );
    free_c_string_array(
        &mut capability.required_parsers,
        capability.required_parsers_len,
    );
    free_c_string_array(
        &mut capability.required_bitstream_filters,
        capability.required_bitstream_filters_len,
    );
    free_c_string(&mut capability.required_tls);
    *capability = PlayerFfiPluginSourceNormalizerCapabilitySummary::default();
}

pub(crate) fn free_plugin_capability(capability: &mut PlayerFfiPluginCapabilitySummary) {
    free_plugin_decoder_capability(&mut capability.decoder);
    free_plugin_frame_processor_capability(&mut capability.frame_processor);
    free_plugin_source_normalizer_capability(&mut capability.source_normalizer);
    *capability = PlayerFfiPluginCapabilitySummary::default();
}

pub(crate) fn free_plugin_diagnostic(diagnostic: &mut PlayerFfiPluginDiagnostic) {
    free_c_string(&mut diagnostic.path);
    free_c_string(&mut diagnostic.plugin_name);
    free_c_string(&mut diagnostic.plugin_kind);
    free_c_string(&mut diagnostic.message);
    free_plugin_capability(&mut diagnostic.capability);
    *diagnostic = PlayerFfiPluginDiagnostic::default();
}

pub(crate) fn free_plugin_diagnostics(startup: &mut PlayerFfiStartup) {
    if !startup.plugin_diagnostics.is_null() {
        unsafe {
            let mut boxed = Box::from_raw(ptr::slice_from_raw_parts_mut(
                startup.plugin_diagnostics,
                startup.plugin_diagnostics_len,
            ));
            for diagnostic in boxed.iter_mut() {
                free_plugin_diagnostic(diagnostic);
            }
        }
    }
    startup.plugin_diagnostics = ptr::null_mut();
    startup.plugin_diagnostics_len = 0;
}

pub(crate) fn free_frame_processor_warning(warning: &mut PlayerFfiFrameProcessorWarning) {
    free_c_string(&mut warning.plugin_name);
    free_c_string(&mut warning.input_handle_kind);
    free_c_string(&mut warning.output_handle_kind);
    free_c_string(&mut warning.message);
    *warning = PlayerFfiFrameProcessorWarning::default();
}

pub(crate) fn free_runtime_warning(warning: &mut PlayerFfiRuntimeWarning) {
    free_frame_processor_warning(&mut warning.frame_processor);
    *warning = PlayerFfiRuntimeWarning::default();
}

pub(crate) fn free_video_decode(video_decode: &mut PlayerFfiVideoDecodeInfo) {
    free_c_string(&mut video_decode.hardware_backend);
    free_c_string(&mut video_decode.fallback_reason);
    *video_decode = PlayerFfiVideoDecodeInfo::default();
}

pub(crate) fn free_startup(startup: &mut PlayerFfiStartup) {
    free_audio_output(&mut startup.audio_output);
    free_video_decode(&mut startup.video_decode);
    free_plugin_diagnostics(startup);
    *startup = PlayerFfiStartup::default();
}

pub(crate) fn free_snapshot(snapshot: &mut PlayerFfiSnapshot) {
    free_c_string(&mut snapshot.source_uri);
    free_media_info(&mut snapshot.media_info);
    *snapshot = PlayerFfiSnapshot::default();
}

pub(crate) fn free_video_frame(frame: &mut PlayerFfiVideoFrame) {
    if !frame.bytes.is_null() {
        unsafe {
            drop(Box::from_raw(ptr::slice_from_raw_parts_mut(
                frame.bytes,
                frame.len,
            )));
        }
    }
    *frame = PlayerFfiVideoFrame::default();
}

pub(crate) fn free_event(event: &mut PlayerFfiEvent) {
    free_startup(&mut event.initialized);
    free_media_info(&mut event.metadata_ready);
    free_audio_output(&mut event.audio_output);
    free_runtime_warning(&mut event.warning);
    unsafe { player_ffi_error_free(&mut event.error) };
    *event = PlayerFfiEvent::default();
}
