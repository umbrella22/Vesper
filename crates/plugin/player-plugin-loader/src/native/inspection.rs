use super::*;

impl NativePlugin {
    /// Validates the callable prefix of a known interface without opening a
    /// media session. Unknown UUIDs are reported without interpreting their tables.
    pub fn inspect_interface(&self, id: VesperInterfaceId) -> PluginResult<bool> {
        macro_rules! require_callbacks {
            ($table:ty, $($field:ident),+ $(,)?) => {{
                let raw = self.query(&id)?.cast::<$table>();
                // SAFETY: this branch is selected only for the UUID of this ABI
                // table. The suffix consists of nullable callback pointers.
                let table = unsafe { read_vtable(raw, offset_of!($table, get_last_error)) }?;
                if $(table.$field.is_none())||+ {
                    return Err(PluginError::Contract("incomplete native interface table"));
                }
            }};
        }

        match id {
            VESPER_AUDIO_PROCESSOR_ID => {
                self.audio_processor(DEFAULT_AUDIO_CALL_BUDGET)?;
            }
            VESPER_POST_DOWNLOAD_PROCESSOR_ID => {
                self.post_download_processor()?;
            }
            VESPER_VIDEO_DECODER_ID => require_callbacks!(
                VesperVideoDecoderVtable,
                open,
                get_format,
                send_packet,
                receive_frame,
                flush,
                destroy,
            ),
            VESPER_VIDEO_FILTER_ID => require_callbacks!(
                VesperVideoFilterVtable,
                open,
                process,
                get_format,
                flush,
                destroy,
            ),
            VESPER_SOURCE_NORMALIZER_ID => require_callbacks!(
                VesperSourceNormalizerVtable,
                open,
                get_stream_info,
                read_packet,
                release_packet,
                seek,
                flush,
                destroy,
            ),
            VESPER_SOURCE_NORMALIZER_RESOURCE_ID => require_callbacks!(
                VesperSourceNormalizerResourceVtable,
                open,
                get_resource_json,
                destroy,
            ),
            _ => return Ok(false),
        }
        Ok(true)
    }
}
