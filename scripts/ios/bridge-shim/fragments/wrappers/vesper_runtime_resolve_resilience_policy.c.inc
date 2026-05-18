{
  if (buffering_policy == NULL || retry_policy == NULL || cache_policy == NULL ||
      out_policy == NULL) {
    return false;
  }

  PlayerFfiBufferingPolicy ffi_buffering_policy = {
      .preset = (PlayerFfiBufferingPreset)buffering_policy->preset_ordinal,
      .has_min_buffer_ms = buffering_policy->has_min_buffer_ms,
      .min_buffer_ms = non_negative_u64(buffering_policy->min_buffer_ms),
      .has_max_buffer_ms = buffering_policy->has_max_buffer_ms,
      .max_buffer_ms = non_negative_u64(buffering_policy->max_buffer_ms),
      .has_buffer_for_playback_ms = buffering_policy->has_buffer_for_playback_ms,
      .buffer_for_playback_ms = non_negative_u64(buffering_policy->buffer_for_playback_ms),
      .has_buffer_for_rebuffer_ms = buffering_policy->has_buffer_for_rebuffer_ms,
      .buffer_for_rebuffer_ms = non_negative_u64(buffering_policy->buffer_for_rebuffer_ms),
  };
  PlayerFfiRetryPolicy ffi_retry_policy = {
      .uses_default_max_attempts = retry_policy->uses_default_max_attempts,
      .has_max_attempts = retry_policy->has_max_attempts,
      .max_attempts =
          retry_policy->max_attempts > 0 ? (uint32_t)retry_policy->max_attempts : 0,
      .has_base_delay_ms = retry_policy->has_base_delay_ms,
      .base_delay_ms = retry_policy->base_delay_ms,
      .has_max_delay_ms = retry_policy->has_max_delay_ms,
      .max_delay_ms = retry_policy->max_delay_ms,
      .has_backoff = retry_policy->has_backoff,
      .backoff = (PlayerFfiRetryBackoff)retry_policy->backoff_ordinal,
  };
  PlayerFfiCachePolicy ffi_cache_policy = {
      .preset = (PlayerFfiCachePreset)cache_policy->preset_ordinal,
      .has_max_memory_bytes = cache_policy->has_max_memory_bytes,
      .max_memory_bytes = non_negative_u64(cache_policy->max_memory_bytes),
      .has_max_disk_bytes = cache_policy->has_max_disk_bytes,
      .max_disk_bytes = non_negative_u64(cache_policy->max_disk_bytes),
  };
  PlayerFfiResolvedResiliencePolicy ffi_resolved_policy;
  PlayerFfiError ffi_error;
  memset(&ffi_resolved_policy, 0, sizeof(ffi_resolved_policy));
  memset(&ffi_error, 0, sizeof(ffi_error));

  PlayerFfiCallStatus status = player_ffi_resolve_resilience_policy(
      (PlayerFfiMediaSourceKind)source_kind_ordinal,
      (PlayerFfiMediaSourceProtocol)source_protocol_ordinal,
      &ffi_buffering_policy,
      &ffi_retry_policy,
      &ffi_cache_policy,
      &ffi_resolved_policy,
      &ffi_error);
  if (status != PlayerFfiCallStatusOk) {
    player_ffi_error_free(&ffi_error);
    return false;
  }

  out_policy->buffering.preset_ordinal = ffi_resolved_policy.buffering.preset;
  out_policy->buffering.has_min_buffer_ms = ffi_resolved_policy.buffering.has_min_buffer_ms;
  out_policy->buffering.min_buffer_ms = (int64_t)ffi_resolved_policy.buffering.min_buffer_ms;
  out_policy->buffering.has_max_buffer_ms = ffi_resolved_policy.buffering.has_max_buffer_ms;
  out_policy->buffering.max_buffer_ms = (int64_t)ffi_resolved_policy.buffering.max_buffer_ms;
  out_policy->buffering.has_buffer_for_playback_ms =
      ffi_resolved_policy.buffering.has_buffer_for_playback_ms;
  out_policy->buffering.buffer_for_playback_ms =
      (int64_t)ffi_resolved_policy.buffering.buffer_for_playback_ms;
  out_policy->buffering.has_buffer_for_rebuffer_ms =
      ffi_resolved_policy.buffering.has_buffer_for_rebuffer_ms;
  out_policy->buffering.buffer_for_rebuffer_ms =
      (int64_t)ffi_resolved_policy.buffering.buffer_for_rebuffer_ms;

  out_policy->retry.uses_default_max_attempts =
      ffi_resolved_policy.retry.uses_default_max_attempts;
  out_policy->retry.has_max_attempts = ffi_resolved_policy.retry.has_max_attempts;
  out_policy->retry.max_attempts = (int32_t)ffi_resolved_policy.retry.max_attempts;
  out_policy->retry.has_base_delay_ms = ffi_resolved_policy.retry.has_base_delay_ms;
  out_policy->retry.base_delay_ms = ffi_resolved_policy.retry.base_delay_ms;
  out_policy->retry.has_max_delay_ms = ffi_resolved_policy.retry.has_max_delay_ms;
  out_policy->retry.max_delay_ms = ffi_resolved_policy.retry.max_delay_ms;
  out_policy->retry.has_backoff = ffi_resolved_policy.retry.has_backoff;
  out_policy->retry.backoff_ordinal = ffi_resolved_policy.retry.backoff;

  out_policy->cache.preset_ordinal = ffi_resolved_policy.cache.preset;
  out_policy->cache.has_max_memory_bytes = ffi_resolved_policy.cache.has_max_memory_bytes;
  out_policy->cache.max_memory_bytes = (int64_t)ffi_resolved_policy.cache.max_memory_bytes;
  out_policy->cache.has_max_disk_bytes = ffi_resolved_policy.cache.has_max_disk_bytes;
  out_policy->cache.max_disk_bytes = (int64_t)ffi_resolved_policy.cache.max_disk_bytes;
  return true;
}
