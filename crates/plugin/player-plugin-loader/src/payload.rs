use super::*;

#[derive(Debug, Error)]
pub(crate) enum PluginPayloadError {
    #[error("plugin payload pointer is null while len is {len}")]
    NullPayloadWithLength { len: usize },
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

pub(crate) fn c_string_field(
    pointer: *const c_char,
    field: &'static str,
) -> Result<String, PluginLoadError> {
    if pointer.is_null() {
        return Err(PluginLoadError::MissingField { field });
    }

    // SAFETY: `pointer` has been checked for null and the plugin ABI requires
    // all string fields to be valid NUL-terminated strings.
    let value = unsafe { CStr::from_ptr(pointer) };
    value
        .to_str()
        .map(|value| value.to_owned())
        .map_err(|_| PluginLoadError::InvalidUtf8 { field })
}

pub(crate) fn map_plugin_payload_error(
    plugin_name: &str,
    payload_kind: &str,
    error: PluginPayloadError,
) -> ProcessorError {
    match error {
        PluginPayloadError::NullPayloadWithLength { len } => ProcessorError::AbiViolation(format!(
            "plugin `{plugin_name}` returned {payload_kind} payload with null data pointer and len {len}"
        )),
        PluginPayloadError::Json(error) => ProcessorError::PayloadCodec(format!(
            "decode plugin `{plugin_name}` {payload_kind} payload failed: {error}"
        )),
    }
}

pub(crate) fn map_capabilities_payload_error(error: PluginPayloadError) -> PluginLoadError {
    match error {
        PluginPayloadError::NullPayloadWithLength { len } => {
            PluginLoadError::CapabilitiesAbiViolation(format!(
                "plugin returned capabilities payload with null data pointer and len {len}"
            ))
        }
        PluginPayloadError::Json(error) => PluginLoadError::DecodeCapabilities(error),
    }
}

pub(crate) fn map_decoder_payload_error(
    plugin_name: &str,
    payload_kind: &str,
    error: PluginPayloadError,
) -> DecoderError {
    match error {
        PluginPayloadError::NullPayloadWithLength { len } => DecoderError::abi_violation(format!(
            "decoder plugin `{plugin_name}` returned {payload_kind} payload with null data pointer and len {len}"
        )),
        PluginPayloadError::Json(error) => DecoderError::payload_codec(format!(
            "decode decoder plugin `{plugin_name}` {payload_kind} payload failed: {error}"
        )),
    }
}

pub(crate) fn decode_decoder_error_payload(
    payload: VesperPluginBytes,
    free_bytes: FreeBytesFn,
    context: *mut c_void,
    plugin_name: &str,
    payload_kind: &str,
) -> DecoderError {
    decode_plugin_bytes::<DecoderError>(payload, free_bytes, context)
        .unwrap_or_else(|error| map_decoder_payload_error(plugin_name, payload_kind, error))
}

pub(crate) fn map_frame_processor_payload_error(
    plugin_name: &str,
    payload_kind: &str,
    error: PluginPayloadError,
) -> FrameProcessorError {
    match error {
        PluginPayloadError::NullPayloadWithLength { len } => {
            FrameProcessorError::abi_violation(format!(
                "frame processor plugin `{plugin_name}` returned {payload_kind} payload with null data pointer and len {len}"
            ))
        }
        PluginPayloadError::Json(error) => FrameProcessorError::payload_codec(format!(
            "decode frame processor plugin `{plugin_name}` {payload_kind} payload failed: {error}"
        )),
    }
}

pub(crate) fn decode_frame_processor_error_payload(
    payload: VesperPluginBytes,
    free_bytes: FreeBytesFn,
    context: *mut c_void,
    plugin_name: &str,
    payload_kind: &str,
) -> FrameProcessorError {
    decode_plugin_bytes::<FrameProcessorError>(payload, free_bytes, context)
        .unwrap_or_else(|error| map_frame_processor_payload_error(plugin_name, payload_kind, error))
}

pub(crate) fn map_source_normalizer_payload_error(
    plugin_name: &str,
    payload_kind: &str,
    error: PluginPayloadError,
) -> SourceNormalizerError {
    match error {
        PluginPayloadError::NullPayloadWithLength { len } => {
            SourceNormalizerError::abi_violation(format!(
                "source normalizer plugin `{plugin_name}` returned {payload_kind} payload with null data pointer and len {len}"
            ))
        }
        PluginPayloadError::Json(error) => SourceNormalizerError::payload_codec(format!(
            "decode source normalizer plugin `{plugin_name}` {payload_kind} payload failed: {error}"
        )),
    }
}

pub(crate) fn decode_source_normalizer_error_payload(
    payload: VesperPluginBytes,
    free_bytes: FreeBytesFn,
    context: *mut c_void,
    plugin_name: &str,
    payload_kind: &str,
) -> SourceNormalizerError {
    decode_plugin_bytes::<SourceNormalizerError>(payload, free_bytes, context).unwrap_or_else(
        |error| map_source_normalizer_payload_error(plugin_name, payload_kind, error),
    )
}

pub(crate) fn decode_plugin_bytes_or_default<T: Default + DeserializeOwned>(
    payload: VesperPluginBytes,
    free_bytes: FreeBytesFn,
    context: *mut c_void,
) -> Result<T, PluginPayloadError> {
    if payload.data.is_null() && payload.len == 0 {
        // SAFETY: this is a no-op for the null/empty payload and keeps the
        // ownership rule symmetric for all plugin-returned buffers.
        unsafe { free_bytes(context, payload) };
        return Ok(T::default());
    }
    decode_plugin_bytes(payload, free_bytes, context)
}

pub(crate) fn reclaim_plugin_payload(
    payload: VesperPluginBytes,
    free_bytes: FreeBytesFn,
    context: *mut c_void,
) {
    // SAFETY: `free_bytes` is the validated deallocator paired with this
    // payload, and the payload is intentionally discarded.
    unsafe { free_bytes(context, payload) };
}

pub(crate) fn decode_plugin_bytes<T: DeserializeOwned>(
    payload: VesperPluginBytes,
    free_bytes: FreeBytesFn,
    context: *mut c_void,
) -> Result<T, PluginPayloadError> {
    let payload_has_null_data = payload.data.is_null();
    let bytes = if payload_has_null_data || payload.len == 0 {
        Vec::new()
    } else {
        // SAFETY: the plugin ABI requires non-null payloads to point to
        // `payload.len` initialized bytes until `free_bytes` is called.
        let slice = unsafe { std::slice::from_raw_parts(payload.data, payload.len) };
        slice.to_vec()
    };

    // SAFETY: `free_bytes` is the validated deallocator paired with this
    // payload, and the payload is not used again after this call.
    unsafe { free_bytes(context, payload) };

    if payload_has_null_data && payload.len > 0 {
        return Err(PluginPayloadError::NullPayloadWithLength { len: payload.len });
    }

    serde_json::from_slice(&bytes).map_err(Into::into)
}
