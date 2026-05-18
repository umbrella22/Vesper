static uint64_t non_negative_u64(int64_t value) {
  return value > 0 ? (uint64_t)value : 0;
}

static uint32_t non_negative_u32(int32_t value) {
  return value > 0 ? (uint32_t)value : 0;
}

static char *duplicate_string(const char *value) {
  if (value == NULL) {
    return NULL;
  }
  return strdup(value);
}

static bool can_allocate_items(uintptr_t len, size_t item_size) {
  return item_size > 0 && len <= (uintptr_t)(SIZE_MAX / item_size);
}

static bool duplicate_runtime_string(const char *value, char **out_value) {
  if (out_value == NULL) {
    return false;
  }
  *out_value = NULL;
  if (value == NULL) {
    return true;
  }
  *out_value = duplicate_string(value);
  return *out_value != NULL;
}

static void free_runtime_string_list(char **values, uintptr_t len) {
  if (values == NULL) {
    return;
  }
  for (uintptr_t index = 0; index < len; index += 1) {
    free(values[index]);
  }
  free(values);
}

static bool copy_ffi_string_list_to_runtime(
    char **values,
    uintptr_t len,
    char ***out_values) {
  if (out_values == NULL) {
    return false;
  }
  *out_values = NULL;
  if (len == 0) {
    return true;
  }
  if (values == NULL || !can_allocate_items(len, sizeof(char *))) {
    return false;
  }

  char **copied_values = calloc((size_t)len, sizeof(char *));
  if (copied_values == NULL) {
    return false;
  }

  for (uintptr_t index = 0; index < len; index += 1) {
    if (!duplicate_runtime_string(values[index], &copied_values[index])) {
      free_runtime_string_list(copied_values, len);
      return false;
    }
  }

  *out_values = copied_values;
  return true;
}

static PlayerFfiDownloadByteRange ffi_download_byte_range_from_runtime(
    VesperRuntimeDownloadByteRange byte_range) {
  PlayerFfiDownloadByteRange ffi_byte_range = {
      .offset = byte_range.offset,
      .length = byte_range.length,
  };
  return ffi_byte_range;
}

static VesperRuntimeDownloadByteRange runtime_download_byte_range_from_ffi(
    PlayerFfiDownloadByteRange byte_range) {
  VesperRuntimeDownloadByteRange runtime_byte_range = {
      .offset = byte_range.offset,
      .length = byte_range.length,
  };
  return runtime_byte_range;
}

static PlayerFfiDownloadConfig ffi_download_config_from_runtime(
    const VesperRuntimeDownloadConfig *config) {
  PlayerFfiDownloadConfig ffi_config = {
      .auto_start = config->auto_start,
      .run_post_processors_on_completion = config->run_post_processors_on_completion,
      .plugin_library_paths = config->plugin_library_paths,
      .plugin_library_paths_len = config->plugin_library_paths_len,
  };
  return ffi_config;
}

static PlayerFfiDownloadSource ffi_download_source_from_runtime(
    const VesperRuntimeDownloadSource *source) {
  PlayerFfiDownloadSource ffi_source = {
      .source_uri = source->source_uri,
      .content_format = (PlayerFfiDownloadContentFormat)source->content_format,
      .manifest_uri = source->manifest_uri,
      .header_names = source->header_names,
      .header_values = source->header_values,
      .headers_len = source->headers_len,
  };
  return ffi_source;
}

static PlayerFfiDownloadProfile ffi_download_profile_from_runtime(
    const VesperRuntimeDownloadProfile *profile) {
  PlayerFfiDownloadProfile ffi_profile = {
      .variant_id = profile->variant_id,
      .preferred_audio_language = profile->preferred_audio_language,
      .preferred_subtitle_language = profile->preferred_subtitle_language,
      .selected_track_ids = profile->selected_track_ids,
      .selected_track_ids_len = profile->selected_track_ids_len,
      .has_target_output_format = profile->has_target_output_format,
      .target_output_format = (PlayerFfiDownloadOutputFormat)profile->target_output_format,
      .target_directory = profile->target_directory,
      .allow_metered_network = profile->allow_metered_network,
  };
  return ffi_profile;
}

static PlayerFfiDownloadResourceRecord ffi_download_resource_from_runtime(
    const VesperRuntimeDownloadResourceRecord *resource) {
  PlayerFfiDownloadResourceRecord ffi_resource = {
      .resource_id = resource->resource_id,
      .uri = resource->uri,
      .relative_path = resource->relative_path,
      .has_byte_range = resource->has_byte_range,
      .byte_range = ffi_download_byte_range_from_runtime(resource->byte_range),
      .generated_text = resource->generated_text,
      .has_size_bytes = resource->has_size_bytes,
      .size_bytes = resource->size_bytes,
      .etag = resource->etag,
      .checksum = resource->checksum,
  };
  return ffi_resource;
}

static PlayerFfiDownloadSegmentRecord ffi_download_segment_from_runtime(
    const VesperRuntimeDownloadSegmentRecord *segment) {
  PlayerFfiDownloadSegmentRecord ffi_segment = {
      .segment_id = segment->segment_id,
      .uri = segment->uri,
      .relative_path = segment->relative_path,
      .has_sequence = segment->has_sequence,
      .sequence = segment->sequence,
      .has_byte_range = segment->has_byte_range,
      .byte_range = ffi_download_byte_range_from_runtime(segment->byte_range),
      .has_size_bytes = segment->has_size_bytes,
      .size_bytes = segment->size_bytes,
      .checksum = segment->checksum,
  };
  return ffi_segment;
}

static PlayerFfiDownloadAssetStream ffi_download_stream_from_runtime(
    const VesperRuntimeDownloadAssetStream *stream) {
  PlayerFfiDownloadAssetStream ffi_stream = {
      .stream_id = stream->stream_id,
      .kind = (PlayerFfiDownloadStreamKind)stream->kind,
      .language = stream->language,
      .codec = stream->codec,
      .label = stream->label,
      .has_quality_rank = stream->has_quality_rank,
      .quality_rank = stream->quality_rank,
      .resource_ids = stream->resource_ids,
      .resource_ids_len = stream->resource_ids_len,
      .segment_ids = stream->segment_ids,
      .segment_ids_len = stream->segment_ids_len,
      .metadata_keys = stream->metadata_keys,
      .metadata_values = stream->metadata_values,
      .metadata_len = stream->metadata_len,
  };
  return ffi_stream;
}

static void free_borrowed_ffi_download_asset_index(PlayerFfiDownloadAssetIndex *asset_index) {
  if (asset_index == NULL) {
    return;
  }
  free(asset_index->resources);
  free(asset_index->segments);
  free(asset_index->streams);
  memset(asset_index, 0, sizeof(*asset_index));
}

static bool copy_runtime_download_resources_to_ffi(
    const VesperRuntimeDownloadResourceRecord *resources,
    uintptr_t len,
    PlayerFfiDownloadResourceRecord **out_resources) {
  if (out_resources == NULL) {
    return false;
  }
  *out_resources = NULL;
  if (len == 0) {
    return true;
  }
  if (resources == NULL || !can_allocate_items(len, sizeof(PlayerFfiDownloadResourceRecord))) {
    return false;
  }

  PlayerFfiDownloadResourceRecord *ffi_resources =
      calloc((size_t)len, sizeof(PlayerFfiDownloadResourceRecord));
  if (ffi_resources == NULL) {
    return false;
  }
  for (uintptr_t index = 0; index < len; index += 1) {
    ffi_resources[index] = ffi_download_resource_from_runtime(&resources[index]);
  }
  *out_resources = ffi_resources;
  return true;
}

static bool copy_runtime_download_segments_to_ffi(
    const VesperRuntimeDownloadSegmentRecord *segments,
    uintptr_t len,
    PlayerFfiDownloadSegmentRecord **out_segments) {
  if (out_segments == NULL) {
    return false;
  }
  *out_segments = NULL;
  if (len == 0) {
    return true;
  }
  if (segments == NULL || !can_allocate_items(len, sizeof(PlayerFfiDownloadSegmentRecord))) {
    return false;
  }

  PlayerFfiDownloadSegmentRecord *ffi_segments =
      calloc((size_t)len, sizeof(PlayerFfiDownloadSegmentRecord));
  if (ffi_segments == NULL) {
    return false;
  }
  for (uintptr_t index = 0; index < len; index += 1) {
    ffi_segments[index] = ffi_download_segment_from_runtime(&segments[index]);
  }
  *out_segments = ffi_segments;
  return true;
}

static bool copy_runtime_download_streams_to_ffi(
    const VesperRuntimeDownloadAssetStream *streams,
    uintptr_t len,
    PlayerFfiDownloadAssetStream **out_streams) {
  if (out_streams == NULL) {
    return false;
  }
  *out_streams = NULL;
  if (len == 0) {
    return true;
  }
  if (streams == NULL || !can_allocate_items(len, sizeof(PlayerFfiDownloadAssetStream))) {
    return false;
  }

  PlayerFfiDownloadAssetStream *ffi_streams =
      calloc((size_t)len, sizeof(PlayerFfiDownloadAssetStream));
  if (ffi_streams == NULL) {
    return false;
  }
  for (uintptr_t index = 0; index < len; index += 1) {
    ffi_streams[index] = ffi_download_stream_from_runtime(&streams[index]);
  }
  *out_streams = ffi_streams;
  return true;
}

static bool ffi_download_asset_index_from_runtime(
    const VesperRuntimeDownloadAssetIndex *asset_index,
    PlayerFfiDownloadAssetIndex *out_asset_index) {
  if (asset_index == NULL || out_asset_index == NULL) {
    return false;
  }
  memset(out_asset_index, 0, sizeof(*out_asset_index));

  out_asset_index->content_format = (PlayerFfiDownloadContentFormat)asset_index->content_format;
  out_asset_index->version = asset_index->version;
  out_asset_index->etag = asset_index->etag;
  out_asset_index->checksum = asset_index->checksum;
  out_asset_index->has_total_size_bytes = asset_index->has_total_size_bytes;
  out_asset_index->total_size_bytes = asset_index->total_size_bytes;
  out_asset_index->resources_len = asset_index->resources_len;
  out_asset_index->segments_len = asset_index->segments_len;
  out_asset_index->streams_len = asset_index->streams_len;
  out_asset_index->completed_path = asset_index->completed_path;

  if (!copy_runtime_download_resources_to_ffi(
          asset_index->resources,
          asset_index->resources_len,
          &out_asset_index->resources) ||
      !copy_runtime_download_segments_to_ffi(
          asset_index->segments,
          asset_index->segments_len,
          &out_asset_index->segments) ||
      !copy_runtime_download_streams_to_ffi(
          asset_index->streams,
          asset_index->streams_len,
          &out_asset_index->streams)) {
    free_borrowed_ffi_download_asset_index(out_asset_index);
    return false;
  }
  return true;
}

static PlayerFfiDownloadProgressSnapshot ffi_download_progress_from_runtime(
    VesperRuntimeDownloadProgressSnapshot progress) {
  PlayerFfiDownloadProgressSnapshot ffi_progress = {
      .received_bytes = progress.received_bytes,
      .has_total_bytes = progress.has_total_bytes,
      .total_bytes = progress.total_bytes,
      .received_segments = progress.received_segments,
      .has_total_segments = progress.has_total_segments,
      .total_segments = progress.total_segments,
  };
  return ffi_progress;
}

static void free_borrowed_ffi_download_task(PlayerFfiDownloadTask *task) {
  if (task == NULL) {
    return;
  }
  free_borrowed_ffi_download_asset_index(&task->asset_index);
  memset(task, 0, sizeof(*task));
}

static bool ffi_download_task_from_runtime(
    const VesperRuntimeDownloadTask *task,
    PlayerFfiDownloadTask *out_task) {
  if (task == NULL || out_task == NULL) {
    return false;
  }
  memset(out_task, 0, sizeof(*out_task));

  out_task->task_id = task->task_id;
  out_task->asset_id = task->asset_id;
  out_task->source = ffi_download_source_from_runtime(&task->source);
  out_task->profile = ffi_download_profile_from_runtime(&task->profile);
  out_task->status = (PlayerFfiDownloadTaskStatus)task->status;
  out_task->progress = ffi_download_progress_from_runtime(task->progress);
  out_task->has_error = task->has_error;
  out_task->error_code = task->error_code;
  out_task->error_category = task->error_category;
  out_task->error_retriable = task->error_retriable;
  out_task->error_message = task->error_message;
  if (!ffi_download_asset_index_from_runtime(&task->asset_index, &out_task->asset_index)) {
    free_borrowed_ffi_download_task(out_task);
    return false;
  }
  return true;
}

static void free_borrowed_ffi_download_tasks(PlayerFfiDownloadTask *tasks, uintptr_t len) {
  if (tasks == NULL) {
    return;
  }
  for (uintptr_t index = 0; index < len; index += 1) {
    free_borrowed_ffi_download_task(&tasks[index]);
  }
  free(tasks);
}

static bool copy_runtime_download_tasks_to_ffi(
    const VesperRuntimeDownloadTask *tasks,
    uintptr_t len,
    PlayerFfiDownloadTask **out_tasks) {
  if (out_tasks == NULL) {
    return false;
  }
  *out_tasks = NULL;
  if (len == 0) {
    return true;
  }
  if (tasks == NULL || !can_allocate_items(len, sizeof(PlayerFfiDownloadTask))) {
    return false;
  }

  PlayerFfiDownloadTask *ffi_tasks = calloc((size_t)len, sizeof(PlayerFfiDownloadTask));
  if (ffi_tasks == NULL) {
    return false;
  }
  for (uintptr_t index = 0; index < len; index += 1) {
    if (!ffi_download_task_from_runtime(&tasks[index], &ffi_tasks[index])) {
      free_borrowed_ffi_download_tasks(ffi_tasks, len);
      return false;
    }
  }
  *out_tasks = ffi_tasks;
  return true;
}

static void free_runtime_download_source_strings(VesperRuntimeDownloadSource *source) {
  if (source == NULL) {
    return;
  }
  uintptr_t headers_len = source->headers_len;
  free(source->source_uri);
  free(source->manifest_uri);
  free_runtime_string_list(source->header_names, headers_len);
  free_runtime_string_list(source->header_values, headers_len);
  memset(source, 0, sizeof(*source));
}

static void free_runtime_download_profile_strings(VesperRuntimeDownloadProfile *profile) {
  if (profile == NULL) {
    return;
  }
  free(profile->variant_id);
  free(profile->preferred_audio_language);
  free(profile->preferred_subtitle_language);
  free_runtime_string_list(profile->selected_track_ids, profile->selected_track_ids_len);
  free(profile->target_directory);
  memset(profile, 0, sizeof(*profile));
}

static void free_runtime_download_resource_strings(VesperRuntimeDownloadResourceRecord *resource) {
  if (resource == NULL) {
    return;
  }
  free(resource->resource_id);
  free(resource->uri);
  free(resource->relative_path);
  free(resource->generated_text);
  free(resource->etag);
  free(resource->checksum);
  memset(resource, 0, sizeof(*resource));
}

static void free_runtime_download_segment_strings(VesperRuntimeDownloadSegmentRecord *segment) {
  if (segment == NULL) {
    return;
  }
  free(segment->segment_id);
  free(segment->uri);
  free(segment->relative_path);
  free(segment->checksum);
  memset(segment, 0, sizeof(*segment));
}

static void free_runtime_download_stream_strings(VesperRuntimeDownloadAssetStream *stream) {
  if (stream == NULL) {
    return;
  }
  free(stream->stream_id);
  free(stream->language);
  free(stream->codec);
  free(stream->label);
  free_runtime_string_list(stream->resource_ids, stream->resource_ids_len);
  free_runtime_string_list(stream->segment_ids, stream->segment_ids_len);
  free_runtime_string_list(stream->metadata_keys, stream->metadata_len);
  free_runtime_string_list(stream->metadata_values, stream->metadata_len);
  memset(stream, 0, sizeof(*stream));
}

static void free_runtime_download_asset_index_strings(VesperRuntimeDownloadAssetIndex *asset_index) {
  if (asset_index == NULL) {
    return;
  }
  free(asset_index->version);
  free(asset_index->etag);
  free(asset_index->checksum);
  if (asset_index->resources != NULL) {
    for (uintptr_t index = 0; index < asset_index->resources_len; index += 1) {
      free_runtime_download_resource_strings(&asset_index->resources[index]);
    }
    free(asset_index->resources);
  }
  if (asset_index->segments != NULL) {
    for (uintptr_t index = 0; index < asset_index->segments_len; index += 1) {
      free_runtime_download_segment_strings(&asset_index->segments[index]);
    }
    free(asset_index->segments);
  }
  if (asset_index->streams != NULL) {
    for (uintptr_t index = 0; index < asset_index->streams_len; index += 1) {
      free_runtime_download_stream_strings(&asset_index->streams[index]);
    }
    free(asset_index->streams);
  }
  free(asset_index->completed_path);
  memset(asset_index, 0, sizeof(*asset_index));
}

static void free_runtime_download_task_strings(VesperRuntimeDownloadTask *task) {
  if (task == NULL) {
    return;
  }
  free(task->asset_id);
  free_runtime_download_source_strings(&task->source);
  free_runtime_download_profile_strings(&task->profile);
  free_runtime_download_asset_index_strings(&task->asset_index);
  free(task->error_message);
  memset(task, 0, sizeof(*task));
}

static void free_runtime_download_command_strings(VesperRuntimeDownloadCommand *command) {
  if (command == NULL) {
    return;
  }
  free_runtime_download_task_strings(&command->task);
  memset(command, 0, sizeof(*command));
}

static void free_runtime_download_event_strings(VesperRuntimeDownloadEvent *event) {
  if (event == NULL) {
    return;
  }
  if (event->task != NULL) {
    free_runtime_download_task_strings(event->task);
    free(event->task);
  }
  free(event->state_error_message);
  free(event->state_completed_path);
  memset(event, 0, sizeof(*event));
}

static bool runtime_download_source_from_ffi(
    const PlayerFfiDownloadSource *source,
    VesperRuntimeDownloadSource *out_source) {
  if (source == NULL || out_source == NULL) {
    return false;
  }
  memset(out_source, 0, sizeof(*out_source));
  out_source->content_format = (VesperRuntimeDownloadContentFormat)source->content_format;
  out_source->headers_len = source->headers_len;
  if (!duplicate_runtime_string(source->source_uri, &out_source->source_uri) ||
      !duplicate_runtime_string(source->manifest_uri, &out_source->manifest_uri) ||
      !copy_ffi_string_list_to_runtime(
          source->header_names,
          source->headers_len,
          &out_source->header_names) ||
      !copy_ffi_string_list_to_runtime(
          source->header_values,
          source->headers_len,
          &out_source->header_values)) {
    free_runtime_download_source_strings(out_source);
    return false;
  }
  return true;
}

static bool runtime_download_profile_from_ffi(
    const PlayerFfiDownloadProfile *profile,
    VesperRuntimeDownloadProfile *out_profile) {
  if (profile == NULL || out_profile == NULL) {
    return false;
  }
  memset(out_profile, 0, sizeof(*out_profile));
  out_profile->selected_track_ids_len = profile->selected_track_ids_len;
  out_profile->has_target_output_format = profile->has_target_output_format;
  out_profile->target_output_format =
      (VesperRuntimeDownloadOutputFormat)profile->target_output_format;
  out_profile->allow_metered_network = profile->allow_metered_network;
  if (!duplicate_runtime_string(profile->variant_id, &out_profile->variant_id) ||
      !duplicate_runtime_string(
          profile->preferred_audio_language,
          &out_profile->preferred_audio_language) ||
      !duplicate_runtime_string(
          profile->preferred_subtitle_language,
          &out_profile->preferred_subtitle_language) ||
      !copy_ffi_string_list_to_runtime(
          profile->selected_track_ids,
          profile->selected_track_ids_len,
          &out_profile->selected_track_ids) ||
      !duplicate_runtime_string(profile->target_directory, &out_profile->target_directory)) {
    free_runtime_download_profile_strings(out_profile);
    return false;
  }
  return true;
}

static bool runtime_download_resource_from_ffi(
    const PlayerFfiDownloadResourceRecord *resource,
    VesperRuntimeDownloadResourceRecord *out_resource) {
  if (resource == NULL || out_resource == NULL) {
    return false;
  }
  memset(out_resource, 0, sizeof(*out_resource));
  out_resource->has_byte_range = resource->has_byte_range;
  out_resource->byte_range = runtime_download_byte_range_from_ffi(resource->byte_range);
  out_resource->generated_text = NULL;
  out_resource->has_size_bytes = resource->has_size_bytes;
  out_resource->size_bytes = resource->size_bytes;
  if (!duplicate_runtime_string(resource->resource_id, &out_resource->resource_id) ||
      !duplicate_runtime_string(resource->uri, &out_resource->uri) ||
      !duplicate_runtime_string(resource->relative_path, &out_resource->relative_path) ||
      !duplicate_runtime_string(resource->etag, &out_resource->etag) ||
      !duplicate_runtime_string(resource->checksum, &out_resource->checksum)) {
    free_runtime_download_resource_strings(out_resource);
    return false;
  }
  return true;
}

static bool runtime_download_segment_from_ffi(
    const PlayerFfiDownloadSegmentRecord *segment,
    VesperRuntimeDownloadSegmentRecord *out_segment) {
  if (segment == NULL || out_segment == NULL) {
    return false;
  }
  memset(out_segment, 0, sizeof(*out_segment));
  out_segment->has_sequence = segment->has_sequence;
  out_segment->sequence = segment->sequence;
  out_segment->has_byte_range = segment->has_byte_range;
  out_segment->byte_range = runtime_download_byte_range_from_ffi(segment->byte_range);
  out_segment->has_size_bytes = segment->has_size_bytes;
  out_segment->size_bytes = segment->size_bytes;
  if (!duplicate_runtime_string(segment->segment_id, &out_segment->segment_id) ||
      !duplicate_runtime_string(segment->uri, &out_segment->uri) ||
      !duplicate_runtime_string(segment->relative_path, &out_segment->relative_path) ||
      !duplicate_runtime_string(segment->checksum, &out_segment->checksum)) {
    free_runtime_download_segment_strings(out_segment);
    return false;
  }
  return true;
}

static bool runtime_download_stream_from_ffi(
    const PlayerFfiDownloadAssetStream *stream,
    VesperRuntimeDownloadAssetStream *out_stream) {
  if (stream == NULL || out_stream == NULL) {
    return false;
  }
  memset(out_stream, 0, sizeof(*out_stream));
  out_stream->kind = (VesperRuntimeDownloadStreamKind)stream->kind;
  out_stream->has_quality_rank = stream->has_quality_rank;
  out_stream->quality_rank = stream->quality_rank;
  out_stream->resource_ids_len = stream->resource_ids_len;
  out_stream->segment_ids_len = stream->segment_ids_len;
  out_stream->metadata_len = stream->metadata_len;
  if (!duplicate_runtime_string(stream->stream_id, &out_stream->stream_id) ||
      !duplicate_runtime_string(stream->language, &out_stream->language) ||
      !duplicate_runtime_string(stream->codec, &out_stream->codec) ||
      !duplicate_runtime_string(stream->label, &out_stream->label) ||
      !copy_ffi_string_list_to_runtime(
          stream->resource_ids,
          stream->resource_ids_len,
          &out_stream->resource_ids) ||
      !copy_ffi_string_list_to_runtime(
          stream->segment_ids,
          stream->segment_ids_len,
          &out_stream->segment_ids) ||
      !copy_ffi_string_list_to_runtime(
          stream->metadata_keys,
          stream->metadata_len,
          &out_stream->metadata_keys) ||
      !copy_ffi_string_list_to_runtime(
          stream->metadata_values,
          stream->metadata_len,
          &out_stream->metadata_values)) {
    free_runtime_download_stream_strings(out_stream);
    return false;
  }
  return true;
}

static bool copy_ffi_download_resources_to_runtime(
    const PlayerFfiDownloadResourceRecord *resources,
    uintptr_t len,
    VesperRuntimeDownloadResourceRecord **out_resources) {
  if (out_resources == NULL) {
    return false;
  }
  *out_resources = NULL;
  if (len == 0) {
    return true;
  }
  if (resources == NULL || !can_allocate_items(len, sizeof(VesperRuntimeDownloadResourceRecord))) {
    return false;
  }

  VesperRuntimeDownloadResourceRecord *runtime_resources =
      calloc((size_t)len, sizeof(VesperRuntimeDownloadResourceRecord));
  if (runtime_resources == NULL) {
    return false;
  }
  for (uintptr_t index = 0; index < len; index += 1) {
    if (!runtime_download_resource_from_ffi(&resources[index], &runtime_resources[index])) {
      for (uintptr_t cleanup_index = 0; cleanup_index < len; cleanup_index += 1) {
        free_runtime_download_resource_strings(&runtime_resources[cleanup_index]);
      }
      free(runtime_resources);
      return false;
    }
  }
  *out_resources = runtime_resources;
  return true;
}

static bool copy_ffi_download_segments_to_runtime(
    const PlayerFfiDownloadSegmentRecord *segments,
    uintptr_t len,
    VesperRuntimeDownloadSegmentRecord **out_segments) {
  if (out_segments == NULL) {
    return false;
  }
  *out_segments = NULL;
  if (len == 0) {
    return true;
  }
  if (segments == NULL || !can_allocate_items(len, sizeof(VesperRuntimeDownloadSegmentRecord))) {
    return false;
  }

  VesperRuntimeDownloadSegmentRecord *runtime_segments =
      calloc((size_t)len, sizeof(VesperRuntimeDownloadSegmentRecord));
  if (runtime_segments == NULL) {
    return false;
  }
  for (uintptr_t index = 0; index < len; index += 1) {
    if (!runtime_download_segment_from_ffi(&segments[index], &runtime_segments[index])) {
      for (uintptr_t cleanup_index = 0; cleanup_index < len; cleanup_index += 1) {
        free_runtime_download_segment_strings(&runtime_segments[cleanup_index]);
      }
      free(runtime_segments);
      return false;
    }
  }
  *out_segments = runtime_segments;
  return true;
}

static bool copy_ffi_download_streams_to_runtime(
    const PlayerFfiDownloadAssetStream *streams,
    uintptr_t len,
    VesperRuntimeDownloadAssetStream **out_streams) {
  if (out_streams == NULL) {
    return false;
  }
  *out_streams = NULL;
  if (len == 0) {
    return true;
  }
  if (streams == NULL || !can_allocate_items(len, sizeof(VesperRuntimeDownloadAssetStream))) {
    return false;
  }

  VesperRuntimeDownloadAssetStream *runtime_streams =
      calloc((size_t)len, sizeof(VesperRuntimeDownloadAssetStream));
  if (runtime_streams == NULL) {
    return false;
  }
  for (uintptr_t index = 0; index < len; index += 1) {
    if (!runtime_download_stream_from_ffi(&streams[index], &runtime_streams[index])) {
      for (uintptr_t cleanup_index = 0; cleanup_index < len; cleanup_index += 1) {
        free_runtime_download_stream_strings(&runtime_streams[cleanup_index]);
      }
      free(runtime_streams);
      return false;
    }
  }
  *out_streams = runtime_streams;
  return true;
}

static bool runtime_download_asset_index_from_ffi(
    const PlayerFfiDownloadAssetIndex *asset_index,
    VesperRuntimeDownloadAssetIndex *out_asset_index) {
  if (asset_index == NULL || out_asset_index == NULL) {
    return false;
  }
  memset(out_asset_index, 0, sizeof(*out_asset_index));
  out_asset_index->content_format =
      (VesperRuntimeDownloadContentFormat)asset_index->content_format;
  out_asset_index->has_total_size_bytes = asset_index->has_total_size_bytes;
  out_asset_index->total_size_bytes = asset_index->total_size_bytes;
  out_asset_index->resources_len = asset_index->resources_len;
  out_asset_index->segments_len = asset_index->segments_len;
  out_asset_index->streams_len = asset_index->streams_len;
  if (!duplicate_runtime_string(asset_index->version, &out_asset_index->version) ||
      !duplicate_runtime_string(asset_index->etag, &out_asset_index->etag) ||
      !duplicate_runtime_string(asset_index->checksum, &out_asset_index->checksum) ||
      !copy_ffi_download_resources_to_runtime(
          asset_index->resources,
          asset_index->resources_len,
          &out_asset_index->resources) ||
      !copy_ffi_download_segments_to_runtime(
          asset_index->segments,
          asset_index->segments_len,
          &out_asset_index->segments) ||
      !copy_ffi_download_streams_to_runtime(
          asset_index->streams,
          asset_index->streams_len,
          &out_asset_index->streams) ||
      !duplicate_runtime_string(asset_index->completed_path, &out_asset_index->completed_path)) {
    free_runtime_download_asset_index_strings(out_asset_index);
    return false;
  }
  return true;
}

static VesperRuntimeDownloadProgressSnapshot runtime_download_progress_from_ffi(
    PlayerFfiDownloadProgressSnapshot progress) {
  VesperRuntimeDownloadProgressSnapshot runtime_progress = {
      .received_bytes = progress.received_bytes,
      .has_total_bytes = progress.has_total_bytes,
      .total_bytes = progress.total_bytes,
      .received_segments = progress.received_segments,
      .has_total_segments = progress.has_total_segments,
      .total_segments = progress.total_segments,
  };
  return runtime_progress;
}

static bool runtime_download_task_from_ffi(
    const PlayerFfiDownloadTask *task,
    VesperRuntimeDownloadTask *out_task) {
  if (task == NULL || out_task == NULL) {
    return false;
  }
  memset(out_task, 0, sizeof(*out_task));
  out_task->task_id = task->task_id;
  out_task->status = (VesperRuntimeDownloadTaskStatus)task->status;
  out_task->progress = runtime_download_progress_from_ffi(task->progress);
  out_task->has_error = task->has_error;
  out_task->error_code = task->error_code;
  out_task->error_category = task->error_category;
  out_task->error_retriable = task->error_retriable;
  if (!duplicate_runtime_string(task->asset_id, &out_task->asset_id) ||
      !runtime_download_source_from_ffi(&task->source, &out_task->source) ||
      !runtime_download_profile_from_ffi(&task->profile, &out_task->profile) ||
      !runtime_download_asset_index_from_ffi(&task->asset_index, &out_task->asset_index) ||
      !duplicate_runtime_string(task->error_message, &out_task->error_message)) {
    free_runtime_download_task_strings(out_task);
    return false;
  }
  return true;
}

static bool runtime_download_command_from_ffi(
    const PlayerFfiDownloadCommand *command,
    VesperRuntimeDownloadCommand *out_command) {
  if (command == NULL || out_command == NULL) {
    return false;
  }
  memset(out_command, 0, sizeof(*out_command));
  out_command->kind = (VesperRuntimeDownloadCommandKind)command->kind;
  out_command->task_id = command->task_id;
  if (!runtime_download_task_from_ffi(&command->task, &out_command->task)) {
    free_runtime_download_command_strings(out_command);
    return false;
  }
  return true;
}

static bool runtime_download_event_from_ffi(
    const PlayerFfiDownloadEvent *event,
    VesperRuntimeDownloadEvent *out_event) {
  if (event == NULL || out_event == NULL) {
    return false;
  }
  memset(out_event, 0, sizeof(*out_event));
  out_event->kind = (VesperRuntimeDownloadEventKind)event->kind;
  if (event->kind == PlayerFfiDownloadEventKindCreated ||
      event->kind == PlayerFfiDownloadEventKindAssetIndexUpdated) {
    out_event->task = calloc(1, sizeof(VesperRuntimeDownloadTask));
    if (out_event->task == NULL ||
        !runtime_download_task_from_ffi(&event->task, out_event->task)) {
      free_runtime_download_event_strings(out_event);
      return false;
    }
  } else if (event->kind == PlayerFfiDownloadEventKindStateChanged) {
    out_event->task_id = event->task_id;
    out_event->state_status = (VesperRuntimeDownloadTaskStatus)event->status;
    out_event->state_progress = runtime_download_progress_from_ffi(event->progress);
    out_event->state_has_error = event->has_error;
    out_event->state_error_code = event->error_code;
    out_event->state_error_category = event->error_category;
    out_event->state_error_retriable = event->error_retriable;
    if (!duplicate_runtime_string(event->error_message, &out_event->state_error_message) ||
        !duplicate_runtime_string(event->completed_path, &out_event->state_completed_path)) {
      free_runtime_download_event_strings(out_event);
      return false;
    }
  } else if (event->kind == PlayerFfiDownloadEventKindProgressUpdated) {
    out_event->task_id = event->task_id;
    out_event->progress = runtime_download_progress_from_ffi(event->progress);
  }
  return true;
}
