#![warn(clippy::undocumented_unsafe_blocks)]

use std::borrow::Cow;

use player_plugin::{
    DecoderBitstreamFormat, DecoderCapabilities, DecoderCodecCapability, DecoderError,
    DecoderFrameFormat, DecoderMediaKind, DecoderNativeDeviceContext,
    DecoderNativeDeviceContextKind, DecoderNativeFrame, DecoderNativeHandleKind,
    DecoderNativeRequirements, DecoderPacket, DecoderPacketResult, DecoderReceiveNativeFrameOutput,
    DecoderSessionConfig, DecoderSessionInfo, NativeDecoderPluginFactory, NativeDecoderSession,
    NativeFrameColorMetadata, NativeFrameHdrMetadata, NativeFramePipelineProfile, Plugin,
    PluginBuildError, normalize_decoder_codec_identifier,
};

const PLUGIN_ID: &str = "io.github.umbrella22.vesper.decoder-mediacodec";
const INSTANCE_ID: &str = "io.github.umbrella22.vesper.decoder-mediacodec.native";
const PLUGIN_NAME: &str = "player-decoder-mediacodec";
const MEDIACODEC_SUPPORTED: bool = cfg!(target_os = "android");
const MEDIACODEC_SURFACE_TEXTURE_FORMAT: &str = "mediacodec_surface_texture";
#[cfg(any(test, target_os = "android"))]
const MAX_OUTPUT_DEQUEUE_STEPS_PER_CALL: usize = 16;
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
type MediaStatus = i32;
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
const AMEDIA_OK: MediaStatus = 0;

#[derive(Debug, Default)]
struct MediaCodecDecoderFactory;

impl NativeDecoderPluginFactory for MediaCodecDecoderFactory {
    fn name(&self) -> &str {
        PLUGIN_NAME
    }

    fn capabilities(&self) -> DecoderCapabilities {
        decoder_capabilities()
    }

    fn native_requirements(&self) -> DecoderNativeRequirements {
        decoder_native_requirements()
    }

    fn open_native_session(
        &self,
        config: &DecoderSessionConfig,
    ) -> Result<Box<dyn NativeDecoderSession>, DecoderError> {
        Ok(Box::new(MediaCodecDecoderSession::open(config.clone())?))
    }
}

#[derive(Debug)]
struct MediaCodecDecoderSession {
    #[cfg(target_os = "android")]
    codec: String,
    decoder_implementation_name: String,
    #[cfg(target_os = "android")]
    backend: android_media::AndroidMediaCodecBackend,
    closed: bool,
}

impl MediaCodecDecoderSession {
    fn open(config: DecoderSessionConfig) -> Result<Self, DecoderError> {
        if !MEDIACODEC_SUPPORTED {
            return Err(DecoderError::UnsupportedCapability {
                capability: "android-mediacodec-decoder".to_owned(),
            });
        }
        if config.media_kind != DecoderMediaKind::Video {
            return Err(DecoderError::UnsupportedCapability {
                capability: "video-native-frame-output".to_owned(),
            });
        }
        if !decoder_capabilities().supports_codec(&config.codec, config.media_kind) {
            return Err(DecoderError::UnsupportedCodec {
                codec: config.codec,
            });
        }
        if config.require_cpu_output {
            return Err(DecoderError::UnsupportedCapability {
                capability: "cpu-video-frame-output".to_owned(),
            });
        }
        validate_mediacodec_bitstream_format(config.bitstream_format.as_ref())?;
        let decoder_implementation_name =
            required_mediacodec_decoder_implementation_name(&config)?.to_owned();
        #[cfg(target_os = "android")]
        let backend = {
            let native_window_ptr =
                android_native_window_ptr(config.native_device_context.as_ref())?;
            android_media::AndroidMediaCodecBackend::open(
                &config,
                native_window_ptr,
                &decoder_implementation_name,
            )?
        };
        Ok(Self {
            #[cfg(target_os = "android")]
            codec: config.codec,
            decoder_implementation_name,
            #[cfg(target_os = "android")]
            backend,
            closed: false,
        })
    }

    fn session_info(&self) -> DecoderSessionInfo {
        DecoderSessionInfo {
            decoder_name: Some(self.decoder_implementation_name.clone()),
            selected_hardware_backend: Some("MediaCodec".to_owned()),
            output_format: Some(mediacodec_surface_texture_format()),
        }
    }

    fn send_packet_impl(
        &mut self,
        packet: &DecoderPacket,
        data: &[u8],
    ) -> Result<DecoderPacketResult, DecoderError> {
        if self.closed {
            return Err(DecoderError::NotConfigured);
        }
        if packet.media_kind != DecoderMediaKind::Video {
            return Err(DecoderError::UnsupportedCapability {
                capability: "video-packet-input".to_owned(),
            });
        }
        #[cfg(target_os = "android")]
        return self.backend.send_packet(packet, data);
        #[cfg(not(target_os = "android"))]
        {
            let _ = (packet, data);
            Err(DecoderError::UnsupportedCapability {
                capability: "android-mediacodec-decoder".to_owned(),
            })
        }
    }

    fn receive_native_frame_impl(
        &mut self,
    ) -> Result<DecoderReceiveNativeFrameOutput, DecoderError> {
        if self.closed {
            return Err(DecoderError::NotConfigured);
        }
        #[cfg(target_os = "android")]
        {
            self.backend.receive_native_frame(&self.codec)
        }
        #[cfg(not(target_os = "android"))]
        {
            Ok(DecoderReceiveNativeFrameOutput::NeedMoreInput)
        }
    }

    fn release_native_frame_impl(
        &mut self,
        frame: DecoderNativeFrame,
        presented: bool,
    ) -> Result<(), DecoderError> {
        if self.closed {
            return Err(DecoderError::NotConfigured);
        }
        if frame.metadata.handle_kind != DecoderNativeHandleKind::MediaCodecSurfaceTexture {
            return Err(DecoderError::abi_violation(format!(
                "MediaCodec plugin expected SurfaceTexture handle kind, got {:?}",
                frame.metadata.handle_kind
            )));
        }
        if frame.handle == 0 {
            return Err(DecoderError::abi_violation(
                "MediaCodec plugin received a null native frame handle",
            ));
        }
        let frame_id = frame.metadata.frame_id.ok_or_else(|| {
            DecoderError::abi_violation("MediaCodec plugin release is missing its frame id")
        })?;
        let handle_frame_id = u64::try_from(frame.handle).map_err(|_| {
            DecoderError::abi_violation("MediaCodec frame handle does not fit its frame id")
        })?;
        if frame_id != handle_frame_id {
            return Err(DecoderError::abi_violation(
                "MediaCodec frame handle does not match its frame id",
            ));
        }
        #[cfg(target_os = "android")]
        return self.backend.release_native_frame(frame.handle, presented);
        #[cfg(not(target_os = "android"))]
        {
            let _ = presented;
            Ok(())
        }
    }

    fn flush_impl(&mut self) -> Result<(), DecoderError> {
        if self.closed {
            return Err(DecoderError::NotConfigured);
        }
        #[cfg(target_os = "android")]
        return self.backend.flush();
        #[cfg(not(target_os = "android"))]
        Ok(())
    }

    fn close_impl(&mut self) -> Result<(), DecoderError> {
        if self.closed {
            return Ok(());
        }
        #[cfg(target_os = "android")]
        if let Err(error) = self.backend.close() {
            self.closed = true;
            return Err(error);
        }
        self.closed = true;
        Ok(())
    }
}

impl NativeDecoderSession for MediaCodecDecoderSession {
    fn session_info(&self) -> DecoderSessionInfo {
        MediaCodecDecoderSession::session_info(self)
    }

    fn send_packet(
        &mut self,
        packet: &DecoderPacket,
        data: &[u8],
    ) -> Result<DecoderPacketResult, DecoderError> {
        self.send_packet_impl(packet, data)
    }

    fn receive_native_frame(&mut self) -> Result<DecoderReceiveNativeFrameOutput, DecoderError> {
        self.receive_native_frame_impl()
    }

    fn release_native_frame(&mut self, frame: DecoderNativeFrame) -> Result<(), DecoderError> {
        self.release_native_frame_impl(frame, false)
    }

    fn release_native_frame_with_presentation(
        &mut self,
        frame: DecoderNativeFrame,
        presented: bool,
    ) -> Result<(), DecoderError> {
        self.release_native_frame_impl(frame, presented)
    }

    fn flush(&mut self) -> Result<(), DecoderError> {
        self.flush_impl()
    }

    fn close(&mut self) -> Result<(), DecoderError> {
        self.close_impl()
    }
}

impl Drop for MediaCodecDecoderSession {
    fn drop(&mut self) {
        let _ = self.close_impl();
    }
}

#[player_plugin::export]
fn mediacodec_decoder_plugin() -> Result<Plugin, PluginBuildError> {
    Plugin::builder(PLUGIN_ID, PLUGIN_NAME)?
        .with_native_decoder(INSTANCE_ID, MediaCodecDecoderFactory)?
        .build()
}

fn decoder_capabilities() -> DecoderCapabilities {
    DecoderCapabilities {
        codecs: if MEDIACODEC_SUPPORTED {
            vec![
                video_codec_capability("H264"),
                video_codec_capability("AVC"),
                video_codec_capability("AVC1"),
                video_codec_capability("AVC3"),
                video_codec_capability("HEVC"),
                video_codec_capability("H265"),
                video_codec_capability("HVC1"),
                video_codec_capability("HEV1"),
            ]
        } else {
            Vec::new()
        },
        supports_hardware_decode: MEDIACODEC_SUPPORTED,
        supports_cpu_video_frames: false,
        supports_audio_frames: false,
        supports_pcm_frames: false,
        supports_gpu_handles: MEDIACODEC_SUPPORTED,
        supports_presentation_release: MEDIACODEC_SUPPORTED,
        supports_flush: true,
        supports_drain: true,
        max_sessions: None,
    }
}

fn decoder_native_requirements() -> DecoderNativeRequirements {
    DecoderNativeRequirements {
        required_device_context_kinds: vec![DecoderNativeDeviceContextKind::AndroidNativeWindow],
        output_handle_kinds: vec![DecoderNativeHandleKind::MediaCodecSurfaceTexture],
        output_pipeline_profiles: vec![NativeFramePipelineProfile::MediaCodecSurfaceTexture],
        requires_native_device_context: true,
        accepted_bitstream_formats: vec![
            DecoderBitstreamFormat::AnnexB,
            DecoderBitstreamFormat::Avcc,
            DecoderBitstreamFormat::Hvcc,
        ],
    }
}

fn video_codec_capability(codec: &str) -> DecoderCodecCapability {
    DecoderCodecCapability {
        codec: codec.to_owned(),
        media_kind: DecoderMediaKind::Video,
        profiles: Vec::new(),
        output_formats: vec![mediacodec_surface_texture_format()],
    }
}

fn mediacodec_surface_texture_format() -> DecoderFrameFormat {
    DecoderFrameFormat::Unknown(MEDIACODEC_SURFACE_TEXTURE_FORMAT.to_owned())
}

fn validate_mediacodec_bitstream_format(
    bitstream_format: Option<&DecoderBitstreamFormat>,
) -> Result<(), DecoderError> {
    match bitstream_format {
        Some(
            DecoderBitstreamFormat::AnnexB
            | DecoderBitstreamFormat::Avcc
            | DecoderBitstreamFormat::Hvcc,
        ) => Ok(()),
        Some(DecoderBitstreamFormat::Unknown(_)) | None => {
            Err(DecoderError::UnsupportedCapability {
                capability: "explicit-annex-b-avcc-or-hvcc-bitstream-format".to_owned(),
            })
        }
    }
}

fn required_mediacodec_decoder_implementation_name(
    config: &DecoderSessionConfig,
) -> Result<&str, DecoderError> {
    config
        .required_decoder_implementation_name
        .as_deref()
        .filter(|name| !name.trim().is_empty())
        .ok_or_else(|| DecoderError::UnsupportedCapability {
            capability: "host-selected-hardware-decoder-implementation".to_owned(),
        })
}

#[cfg(any(test, target_os = "android"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MediaCodecDequeueStep<T> {
    Yield(T),
    Skip,
}

#[cfg(any(test, target_os = "android"))]
fn run_bounded_mediacodec_dequeue<T, E>(
    mut dequeue_step: impl FnMut() -> Result<MediaCodecDequeueStep<T>, E>,
) -> Result<Option<T>, E> {
    for _ in 0..MAX_OUTPUT_DEQUEUE_STEPS_PER_CALL {
        match dequeue_step()? {
            MediaCodecDequeueStep::Yield(output) => return Ok(Some(output)),
            MediaCodecDequeueStep::Skip => {}
        }
    }
    Ok(None)
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn android_native_window_ptr(
    context: Option<&DecoderNativeDeviceContext>,
) -> Result<usize, DecoderError> {
    let Some(context) = context else {
        return Err(DecoderError::UnsupportedCapability {
            capability: "android-native-window-device-context".to_owned(),
        });
    };
    let Some(window_ptr) = context.android_native_window_ptr() else {
        return Err(DecoderError::UnsupportedCapability {
            capability: "android-native-window-device-context".to_owned(),
        });
    };
    if window_ptr == 0 {
        return Err(DecoderError::abi_violation(
            "Android native window device context pointer is null",
        ));
    }
    Ok(window_ptr)
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn codec_mime(codec: &str) -> Option<&'static str> {
    let normalized = normalize_decoder_codec_identifier(codec);
    if matches!(normalized.as_str(), "h264" | "avc" | "avc1" | "avc3") {
        Some("video/avc")
    } else if matches!(normalized.as_str(), "hevc" | "h265" | "hvc1" | "hev1") {
        Some("video/hevc")
    } else {
        None
    }
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn split_avcc_extradata(extradata: &[u8]) -> Vec<Vec<u8>> {
    if extradata.len() < 7 || extradata[0] != 1 {
        return vec![extradata.to_vec()];
    }
    let mut offset = 5;
    let sps_count = extradata[offset] & 0x1f;
    offset += 1;
    let mut units = Vec::new();
    for _ in 0..sps_count {
        let Some(length) = read_be_u16(extradata, offset) else {
            return vec![extradata.to_vec()];
        };
        offset += 2;
        let end = offset.saturating_add(usize::from(length));
        if end > extradata.len() {
            return vec![extradata.to_vec()];
        }
        units.push(with_annex_b_start_code(&extradata[offset..end]));
        offset = end;
    }
    if offset >= extradata.len() {
        return units;
    }
    let pps_count = extradata[offset];
    offset += 1;
    for _ in 0..pps_count {
        let Some(length) = read_be_u16(extradata, offset) else {
            return vec![extradata.to_vec()];
        };
        offset += 2;
        let end = offset.saturating_add(usize::from(length));
        if end > extradata.len() {
            return vec![extradata.to_vec()];
        }
        units.push(with_annex_b_start_code(&extradata[offset..end]));
        offset = end;
    }
    if units.is_empty() {
        vec![extradata.to_vec()]
    } else {
        units
    }
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn split_hvcc_extradata(extradata: &[u8]) -> Vec<Vec<u8>> {
    if extradata.len() < 23 || extradata[0] != 1 {
        return vec![extradata.to_vec()];
    }
    let mut offset = 22;
    let array_count = extradata[offset];
    offset += 1;
    let mut units = Vec::new();
    for _ in 0..array_count {
        if offset + 3 > extradata.len() {
            return vec![extradata.to_vec()];
        }
        offset += 1;
        let Some(nal_count) = read_be_u16(extradata, offset) else {
            return vec![extradata.to_vec()];
        };
        offset += 2;
        for _ in 0..nal_count {
            let Some(length) = read_be_u16(extradata, offset) else {
                return vec![extradata.to_vec()];
            };
            offset += 2;
            let end = offset.saturating_add(usize::from(length));
            if end > extradata.len() {
                return vec![extradata.to_vec()];
            }
            units.push(with_annex_b_start_code(&extradata[offset..end]));
            offset = end;
        }
    }
    if units.is_empty() {
        vec![extradata.to_vec()]
    } else {
        units
    }
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn codec_config_buffers(config: &DecoderSessionConfig) -> Vec<Vec<u8>> {
    if config.extradata.is_empty() {
        return Vec::new();
    }
    match config.bitstream_format.as_ref() {
        Some(DecoderBitstreamFormat::Avcc) => split_avcc_extradata(&config.extradata),
        Some(DecoderBitstreamFormat::Hvcc) => split_hvcc_extradata(&config.extradata),
        _ => vec![config.extradata.clone()],
    }
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn nal_length_size_for_config(config: &DecoderSessionConfig) -> usize {
    match config.bitstream_format.as_ref() {
        Some(DecoderBitstreamFormat::Avcc) => avcc_nal_length_size(&config.extradata),
        Some(DecoderBitstreamFormat::Hvcc) => hvcc_nal_length_size(&config.extradata),
        _ => None,
    }
    .unwrap_or(4)
    .clamp(1, 4)
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn avcc_nal_length_size(extradata: &[u8]) -> Option<usize> {
    if extradata.len() >= 5 && extradata[0] == 1 {
        Some(usize::from((extradata[4] & 0x03) + 1))
    } else {
        None
    }
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn hvcc_nal_length_size(extradata: &[u8]) -> Option<usize> {
    if extradata.len() >= 22 && extradata[0] == 1 {
        Some(usize::from((extradata[21] & 0x03) + 1))
    } else {
        None
    }
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn packet_data_for_mediacodec<'a>(
    bitstream_format: Option<&DecoderBitstreamFormat>,
    nal_length_size: usize,
    data: &'a [u8],
) -> Result<Cow<'a, [u8]>, DecoderError> {
    match bitstream_format {
        Some(DecoderBitstreamFormat::Avcc) | Some(DecoderBitstreamFormat::Hvcc) => {
            if data.is_empty() || has_annex_b_start_code(data) {
                Ok(Cow::Borrowed(data))
            } else {
                length_prefixed_sample_to_annex_b(data, nal_length_size).map(Cow::Owned)
            }
        }
        _ => Ok(Cow::Borrowed(data)),
    }
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn length_prefixed_sample_to_annex_b(
    data: &[u8],
    nal_length_size: usize,
) -> Result<Vec<u8>, DecoderError> {
    let nal_length_size = nal_length_size.clamp(1, 4);
    let mut offset = 0;
    let mut output = Vec::with_capacity(data.len().saturating_add(4));
    while offset < data.len() {
        if offset.saturating_add(nal_length_size) > data.len() {
            return Err(DecoderError::InvalidPacket {
                message: format!(
                    "length-prefixed MediaCodec sample ended inside a {nal_length_size}-byte NAL length field"
                ),
            });
        }
        let mut nal_len = 0_usize;
        for byte in &data[offset..offset + nal_length_size] {
            nal_len = (nal_len << 8) | usize::from(*byte);
        }
        offset += nal_length_size;
        if nal_len == 0 {
            continue;
        }
        let end = offset.saturating_add(nal_len);
        if end > data.len() {
            return Err(DecoderError::InvalidPacket {
                message: format!(
                    "length-prefixed MediaCodec sample declared NAL length {nal_len} beyond packet size"
                ),
            });
        }
        output.extend_from_slice(&[0, 0, 0, 1]);
        output.extend_from_slice(&data[offset..end]);
        offset = end;
    }
    if output.is_empty() && !data.is_empty() {
        return Err(DecoderError::InvalidPacket {
            message: "length-prefixed MediaCodec sample did not contain a NAL unit".to_owned(),
        });
    }
    Ok(output)
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn has_annex_b_start_code(data: &[u8]) -> bool {
    data.starts_with(&[0, 0, 1]) || data.starts_with(&[0, 0, 0, 1])
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn with_annex_b_start_code(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + 4);
    out.extend_from_slice(&[0, 0, 0, 1]);
    out.extend_from_slice(data);
    out
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn read_be_u16(data: &[u8], offset: usize) -> Option<u16> {
    let bytes = data.get(offset..offset + 2)?;
    Some(u16::from_be_bytes([bytes[0], bytes[1]]))
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn packet_input_data_for_mediacodec<'a>(
    packet: &DecoderPacket,
    bitstream_format: Option<&DecoderBitstreamFormat>,
    nal_length_size: usize,
    data: &'a [u8],
) -> Result<Cow<'a, [u8]>, DecoderError> {
    if packet.end_of_stream {
        Ok(Cow::Borrowed(&[]))
    } else {
        packet_data_for_mediacodec(bitstream_format, nal_length_size, data)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
enum MediaCodecOutputBufferKind {
    CodecConfig,
    Eof,
    Frame { pending_eos: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
enum MediaCodecPendingOutputEosAction {
    WaitForOutstandingRelease,
    ReportEof,
}

#[derive(Debug)]
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
struct MediaCodecFrameLeaseIds {
    next: usize,
}

impl Default for MediaCodecFrameLeaseIds {
    fn default() -> Self {
        Self { next: 1 }
    }
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
impl MediaCodecFrameLeaseIds {
    fn allocate(&mut self) -> Result<usize, DecoderError> {
        let lease_id = self.next;
        self.next = lease_id.checked_add(1).ok_or_else(|| {
            DecoderError::internal("MediaCodec native-frame lease id space is exhausted")
        })?;
        Ok(lease_id)
    }
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn complete_mediacodec_frame_release<T>(
    outstanding: &mut std::collections::HashMap<usize, T>,
    handle: usize,
    release_result: Result<(), DecoderError>,
) -> Result<(), DecoderError> {
    release_result?;
    outstanding.remove(&handle);
    Ok(())
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn mediacodec_output_buffer_kind(flags: u32, size: i32) -> MediaCodecOutputBufferKind {
    const CODEC_CONFIG: u32 = 2;
    const END_OF_STREAM: u32 = 4;
    if flags & CODEC_CONFIG != 0 {
        MediaCodecOutputBufferKind::CodecConfig
    } else if flags & END_OF_STREAM != 0 && size <= 0 {
        MediaCodecOutputBufferKind::Eof
    } else {
        MediaCodecOutputBufferKind::Frame {
            pending_eos: flags & END_OF_STREAM != 0,
        }
    }
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn mediacodec_pending_output_eos_action(
    outstanding_frame_count: usize,
) -> MediaCodecPendingOutputEosAction {
    if outstanding_frame_count == 0 {
        MediaCodecPendingOutputEosAction::ReportEof
    } else {
        MediaCodecPendingOutputEosAction::WaitForOutstandingRelease
    }
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn merge_mediacodec_color_metadata(
    fallback: Option<NativeFrameColorMetadata>,
    runtime: Option<NativeFrameColorMetadata>,
) -> Option<NativeFrameColorMetadata> {
    match (fallback, runtime) {
        (Some(fallback), Some(runtime)) => Some(NativeFrameColorMetadata {
            primaries: runtime.primaries.or(fallback.primaries),
            transfer: runtime.transfer.or(fallback.transfer),
            matrix: runtime.matrix.or(fallback.matrix),
            range: runtime.range.or(fallback.range),
            bit_depth: runtime.bit_depth.or(fallback.bit_depth),
        }),
        (Some(fallback), None) => Some(fallback),
        (None, Some(runtime)) => Some(runtime),
        (None, None) => None,
    }
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn mediacodec_hdr_metadata_from_color(
    color: Option<&NativeFrameColorMetadata>,
) -> Option<NativeFrameHdrMetadata> {
    let transfer = color?.transfer.as_deref()?.to_ascii_lowercase();
    let kind = if transfer.contains("st2084") || transfer.contains("smpte2084") {
        "hdr10"
    } else if transfer.contains("hlg") || transfer.contains("arib-std-b67") {
        "hlg"
    } else {
        return None;
    };
    Some(NativeFrameHdrMetadata {
        kind: kind.to_owned(),
        mastering_display: None,
        content_light: None,
        dolby_vision: None,
    })
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn merge_mediacodec_hdr_metadata(
    fallback: Option<NativeFrameHdrMetadata>,
    runtime: Option<NativeFrameHdrMetadata>,
) -> Option<NativeFrameHdrMetadata> {
    match (fallback, runtime) {
        (Some(fallback), Some(runtime)) => {
            if mediacodec_hdr_metadata_has_richer_fields(&fallback) {
                Some(fallback)
            } else {
                Some(NativeFrameHdrMetadata {
                    kind: fallback.kind,
                    mastering_display: fallback.mastering_display.or(runtime.mastering_display),
                    content_light: fallback.content_light.or(runtime.content_light),
                    dolby_vision: fallback.dolby_vision.or(runtime.dolby_vision),
                })
            }
        }
        (Some(fallback), None) => Some(fallback),
        (None, Some(runtime)) => Some(runtime),
        (None, None) => None,
    }
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn mediacodec_hdr_metadata_has_richer_fields(metadata: &NativeFrameHdrMetadata) -> bool {
    metadata.is_dolby_vision()
        || metadata.mastering_display.is_some()
        || metadata.content_light.is_some()
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn android_color_standard_label(value: i32) -> Option<String> {
    match value {
        1 => Some("bt709".to_owned()),
        2 => Some("bt601-pal".to_owned()),
        4 => Some("bt601-ntsc".to_owned()),
        6 => Some("bt2020".to_owned()),
        _ => None,
    }
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn android_color_standard_matrix_label(value: i32) -> Option<String> {
    match value {
        1 => Some("bt709".to_owned()),
        2 | 4 => Some("bt601".to_owned()),
        6 => Some("bt2020-ncl".to_owned()),
        _ => None,
    }
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn android_color_transfer_label(value: i32) -> Option<String> {
    match value {
        1 => Some("linear".to_owned()),
        3 => Some("sdr-video".to_owned()),
        6 => Some("st2084".to_owned()),
        7 => Some("hlg".to_owned()),
        _ => None,
    }
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn android_color_range_label(value: i32) -> Option<String> {
    match value {
        1 => Some("full".to_owned()),
        2 => Some("limited".to_owned()),
        _ => None,
    }
}

#[cfg(target_os = "android")]
mod android_media {
    use std::collections::HashMap;
    use std::ffi::{CStr, CString, c_char, c_void};
    use std::ptr::NonNull;

    use player_plugin::{
        DecoderBitstreamFormat, DecoderError, DecoderMediaKind, DecoderNativeFrame,
        DecoderNativeFrameMetadata, DecoderNativeFrameReleaseTracking, DecoderNativeHandleKind,
        DecoderPacket, DecoderPacketResult, DecoderReceiveNativeFrameOutput, DecoderVisibleRect,
        NativeFrameColorMetadata, NativeFrameHdrMetadata, NativeFramePipelineProfile,
    };

    use super::{
        AMEDIA_OK, MediaCodecDequeueStep, MediaCodecFrameLeaseIds, MediaCodecOutputBufferKind,
        MediaCodecPendingOutputEosAction, MediaStatus, android_color_range_label,
        android_color_standard_label, android_color_standard_matrix_label,
        android_color_transfer_label, codec_config_buffers, codec_mime,
        complete_mediacodec_frame_release, media_codec_close_result,
        mediacodec_hdr_metadata_from_color, mediacodec_output_buffer_kind,
        mediacodec_pending_output_eos_action, mediacodec_surface_texture_format,
        merge_mediacodec_color_metadata, merge_mediacodec_hdr_metadata, nal_length_size_for_config,
        packet_input_data_for_mediacodec, run_bounded_mediacodec_dequeue,
    };

    const AMEDIACODEC_INFO_TRY_AGAIN_LATER: isize = -1;
    const AMEDIACODEC_INFO_OUTPUT_FORMAT_CHANGED: isize = -2;
    const AMEDIACODEC_INFO_OUTPUT_BUFFERS_CHANGED: isize = -3;
    const AMEDIACODEC_BUFFER_FLAG_END_OF_STREAM: u32 = 4;
    const DEQUEUE_TIMEOUT_US: i64 = 0;
    const AMEDIAFORMAT_KEY_CROP_LEFT_FALLBACK: &CStr = c"crop-left";
    const AMEDIAFORMAT_KEY_CROP_RIGHT_FALLBACK: &CStr = c"crop-right";
    const AMEDIAFORMAT_KEY_CROP_TOP_FALLBACK: &CStr = c"crop-top";
    const AMEDIAFORMAT_KEY_CROP_BOTTOM_FALLBACK: &CStr = c"crop-bottom";
    const AMEDIAFORMAT_KEY_COLOR_STANDARD_FALLBACK: &CStr = c"color-standard";
    const AMEDIAFORMAT_KEY_COLOR_TRANSFER_FALLBACK: &CStr = c"color-transfer";
    const AMEDIAFORMAT_KEY_COLOR_RANGE_FALLBACK: &CStr = c"color-range";

    #[repr(C)]
    struct AMediaCodec {
        _private: [u8; 0],
    }

    #[repr(C)]
    struct AMediaFormat {
        _private: [u8; 0],
    }

    #[repr(C)]
    struct ANativeWindow {
        _private: [u8; 0],
    }

    #[repr(C)]
    #[derive(Debug, Clone, Copy, Default)]
    struct AMediaCodecBufferInfo {
        offset: i32,
        size: i32,
        presentation_time_us: i64,
        flags: u32,
    }

    #[link(name = "mediandk")]
    unsafe extern "C" {
        static AMEDIAFORMAT_KEY_MIME: *const c_char;
        static AMEDIAFORMAT_KEY_WIDTH: *const c_char;
        static AMEDIAFORMAT_KEY_HEIGHT: *const c_char;
        static AMEDIAFORMAT_KEY_CSD_0: *const c_char;
        static AMEDIAFORMAT_KEY_CSD_1: *const c_char;
        static AMEDIAFORMAT_KEY_CSD_2: *const c_char;

        fn AMediaFormat_new() -> *mut AMediaFormat;
        fn AMediaFormat_delete(format: *mut AMediaFormat) -> MediaStatus;
        fn AMediaFormat_setString(
            format: *mut AMediaFormat,
            name: *const c_char,
            value: *const c_char,
        );
        fn AMediaFormat_setInt32(format: *mut AMediaFormat, name: *const c_char, value: i32);
        fn AMediaFormat_getInt32(
            format: *const AMediaFormat,
            name: *const c_char,
            out: *mut i32,
        ) -> bool;
        fn AMediaFormat_setBuffer(
            format: *mut AMediaFormat,
            name: *const c_char,
            data: *const c_void,
            size: usize,
        );

        fn AMediaCodec_createCodecByName(name: *const c_char) -> *mut AMediaCodec;
        fn AMediaCodec_configure(
            codec: *mut AMediaCodec,
            format: *const AMediaFormat,
            surface: *mut ANativeWindow,
            crypto: *mut c_void,
            flags: u32,
        ) -> MediaStatus;
        fn AMediaCodec_start(codec: *mut AMediaCodec) -> MediaStatus;
        fn AMediaCodec_stop(codec: *mut AMediaCodec) -> MediaStatus;
        fn AMediaCodec_flush(codec: *mut AMediaCodec) -> MediaStatus;
        fn AMediaCodec_delete(codec: *mut AMediaCodec) -> MediaStatus;
        fn AMediaCodec_dequeueInputBuffer(codec: *mut AMediaCodec, timeout_us: i64) -> isize;
        fn AMediaCodec_getInputBuffer(
            codec: *mut AMediaCodec,
            index: usize,
            out_size: *mut usize,
        ) -> *mut u8;
        fn AMediaCodec_queueInputBuffer(
            codec: *mut AMediaCodec,
            index: usize,
            offset: isize,
            size: usize,
            time_us: u64,
            flags: u32,
        ) -> MediaStatus;
        fn AMediaCodec_dequeueOutputBuffer(
            codec: *mut AMediaCodec,
            info: *mut AMediaCodecBufferInfo,
            timeout_us: i64,
        ) -> isize;
        fn AMediaCodec_getOutputFormat(codec: *mut AMediaCodec) -> *mut AMediaFormat;
        fn AMediaCodec_releaseOutputBuffer(
            codec: *mut AMediaCodec,
            index: usize,
            render: bool,
        ) -> MediaStatus;
    }

    #[derive(Debug)]
    pub(super) struct AndroidMediaCodecBackend {
        codec: MediaCodecHandle,
        output_format: MediaCodecOutputFormat,
        bitstream_format: Option<DecoderBitstreamFormat>,
        nal_length_size: usize,
        frame_lease_ids: MediaCodecFrameLeaseIds,
        outstanding: HashMap<usize, MediaCodecOutputFrame>,
        saw_input_eos: bool,
        saw_output_eos: bool,
        pending_output_eos: bool,
        closed: bool,
    }

    #[derive(Debug, Clone, Copy)]
    struct MediaCodecOutputFrame {
        index: usize,
    }

    #[derive(Debug)]
    struct MediaCodecHandle {
        raw: Option<NonNull<AMediaCodec>>,
    }

    impl MediaCodecHandle {
        fn new(raw: NonNull<AMediaCodec>) -> Self {
            Self { raw: Some(raw) }
        }

        fn as_ptr(&self) -> Result<*mut AMediaCodec, DecoderError> {
            self.raw
                .map(NonNull::as_ptr)
                .ok_or(DecoderError::NotConfigured)
        }

        fn delete(&mut self) -> MediaStatus {
            let Some(raw) = self.raw.take() else {
                return AMEDIA_OK;
            };
            // SAFETY: `raw` is uniquely owned by this wrapper and is removed
            // before the destructive delete call, so it cannot be deleted twice.
            unsafe { AMediaCodec_delete(raw.as_ptr()) }
        }
    }

    // SAFETY: the synchronous NDK MediaCodec API has no caller-thread
    // affinity. This wrapper uniquely owns the handle, never registers async
    // callbacks, and all operations are serialized through mutable backend
    // access. It is intentionally not `Sync`.
    unsafe impl Send for MediaCodecHandle {}

    impl Drop for MediaCodecHandle {
        fn drop(&mut self) {
            let status = self.delete();
            if status != AMEDIA_OK {
                tracing::warn!(
                    media_status = status,
                    "AMediaCodec_delete failed while dropping an owned codec"
                );
            }
        }
    }

    #[allow(dead_code)]
    fn assert_android_backend_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<AndroidMediaCodecBackend>();
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct MediaCodecOutputFormat {
        width: u32,
        height: u32,
        coded_width: u32,
        coded_height: u32,
        visible_rect: DecoderVisibleRect,
        color: Option<NativeFrameColorMetadata>,
        hdr: Option<NativeFrameHdrMetadata>,
    }

    impl MediaCodecOutputFormat {
        fn new(
            width: u32,
            height: u32,
            color: Option<NativeFrameColorMetadata>,
            hdr: Option<NativeFrameHdrMetadata>,
        ) -> Self {
            Self {
                width,
                height,
                coded_width: width,
                coded_height: height,
                visible_rect: DecoderVisibleRect {
                    x: 0,
                    y: 0,
                    width,
                    height,
                },
                color,
                hdr,
            }
        }
    }

    impl AndroidMediaCodecBackend {
        pub(super) fn open(
            config: &player_plugin::DecoderSessionConfig,
            native_window_ptr: usize,
            decoder_implementation_name: &str,
        ) -> Result<Self, DecoderError> {
            let mime = codec_mime(&config.codec).ok_or_else(|| DecoderError::UnsupportedCodec {
                codec: config.codec.clone(),
            })?;
            let mime = CString::new(mime).map_err(|error| {
                DecoderError::payload_codec(format!("invalid MediaCodec MIME string: {error}"))
            })?;
            let decoder_implementation_name =
                CString::new(decoder_implementation_name).map_err(|error| {
                    DecoderError::payload_codec(format!(
                        "invalid MediaCodec implementation name: {error}"
                    ))
                })?;
            let width = config.width.or(config.coded_width).unwrap_or(0);
            let height = config.height.or(config.coded_height).unwrap_or(0);
            if width == 0 || height == 0 {
                return Err(DecoderError::InvalidPacket {
                    message: "MediaCodec video config requires non-zero width and height"
                        .to_owned(),
                });
            }
            let format = MediaFormat::new()?;
            // SAFETY: `format` is a valid AMediaFormat and the key pointers are
            // process-lifetime NDK constants.
            unsafe {
                AMediaFormat_setString(format.as_ptr(), AMEDIAFORMAT_KEY_MIME, mime.as_ptr());
                AMediaFormat_setInt32(format.as_ptr(), AMEDIAFORMAT_KEY_WIDTH, width as i32);
                AMediaFormat_setInt32(format.as_ptr(), AMEDIAFORMAT_KEY_HEIGHT, height as i32);
            }
            for (index, buffer) in codec_config_buffers(config).iter().take(3).enumerate() {
                let key = match index {
                    0 => unsafe { AMEDIAFORMAT_KEY_CSD_0 },
                    1 => unsafe { AMEDIAFORMAT_KEY_CSD_1 },
                    _ => unsafe { AMEDIAFORMAT_KEY_CSD_2 },
                };
                // SAFETY: `buffer` is borrowed for this call only and
                // AMediaFormat copies the provided bytes.
                unsafe {
                    AMediaFormat_setBuffer(
                        format.as_ptr(),
                        key,
                        buffer.as_ptr().cast::<c_void>(),
                        buffer.len(),
                    );
                }
            }
            // SAFETY: the implementation-name CString is valid and
            // NUL-terminated. The returned codec is uniquely owned here.
            let codec = MediaCodecHandle::new(
                NonNull::new(unsafe {
                    AMediaCodec_createCodecByName(decoder_implementation_name.as_ptr())
                })
                .ok_or_else(|| DecoderError::UnsupportedCapability {
                    capability: "host-selected-hardware-decoder-implementation".to_owned(),
                })?,
            );
            let surface = native_window_ptr as *mut ANativeWindow;
            let codec_ptr = codec.as_ptr()?;
            // SAFETY: `codec`, `format`, and `surface` are valid for this call.
            let configure_status = unsafe {
                AMediaCodec_configure(codec_ptr, format.as_ptr(), surface, std::ptr::null_mut(), 0)
            };
            if configure_status != AMEDIA_OK {
                return Err(media_status_error(
                    "AMediaCodec_configure",
                    configure_status,
                ));
            }
            // SAFETY: `codec` is configured and ready to start.
            let start_status = unsafe { AMediaCodec_start(codec_ptr) };
            if start_status != AMEDIA_OK {
                return Err(media_status_error("AMediaCodec_start", start_status));
            }
            Ok(Self {
                codec,
                output_format: MediaCodecOutputFormat::new(
                    width,
                    height,
                    config.color.clone(),
                    config.hdr.clone(),
                ),
                bitstream_format: config.bitstream_format.clone(),
                nal_length_size: nal_length_size_for_config(config),
                frame_lease_ids: MediaCodecFrameLeaseIds::default(),
                outstanding: HashMap::new(),
                saw_input_eos: false,
                saw_output_eos: false,
                pending_output_eos: false,
                closed: false,
            })
        }

        pub(super) fn send_packet(
            &mut self,
            packet: &DecoderPacket,
            data: &[u8],
        ) -> Result<DecoderPacketResult, DecoderError> {
            if self.closed {
                return Err(DecoderError::NotConfigured);
            }
            if self.saw_input_eos {
                return Err(DecoderError::Eof);
            }
            let index = match self.dequeue_input()? {
                Some(index) => index,
                None => return Ok(DecoderPacketResult { accepted: false }),
            };
            let flags = if packet.end_of_stream {
                AMEDIACODEC_BUFFER_FLAG_END_OF_STREAM
            } else {
                0
            };
            let pts_us = packet.pts_us.or(packet.dts_us).unwrap_or_default().max(0) as u64;
            let input_data = packet_input_data_for_mediacodec(
                packet,
                self.bitstream_format.as_ref(),
                self.nal_length_size,
                data,
            )?;
            if !packet.end_of_stream {
                self.copy_input(index, input_data.as_ref())?;
            }
            let codec = self.codec.as_ptr()?;
            // SAFETY: `index` came from dequeueInputBuffer and the copied data
            // length fits that input buffer.
            let status = unsafe {
                AMediaCodec_queueInputBuffer(codec, index, 0, input_data.len(), pts_us, flags)
            };
            if status != AMEDIA_OK {
                return Err(media_status_error("AMediaCodec_queueInputBuffer", status));
            }
            if packet.end_of_stream {
                self.saw_input_eos = true;
            }
            Ok(DecoderPacketResult { accepted: true })
        }

        pub(super) fn receive_native_frame(
            &mut self,
            codec_name: &str,
        ) -> Result<DecoderReceiveNativeFrameOutput, DecoderError> {
            if self.closed {
                return Err(DecoderError::NotConfigured);
            }
            if self.saw_output_eos {
                return Ok(DecoderReceiveNativeFrameOutput::Eof);
            }
            if self.pending_output_eos {
                match mediacodec_pending_output_eos_action(self.outstanding.len()) {
                    MediaCodecPendingOutputEosAction::ReportEof => {
                        self.pending_output_eos = false;
                        self.saw_output_eos = true;
                        return Ok(DecoderReceiveNativeFrameOutput::Eof);
                    }
                    MediaCodecPendingOutputEosAction::WaitForOutstandingRelease => {
                        return Ok(DecoderReceiveNativeFrameOutput::NeedMoreInput);
                    }
                }
            }
            let output = run_bounded_mediacodec_dequeue(|| {
                let mut info = AMediaCodecBufferInfo::default();
                let codec = self.codec.as_ptr()?;
                // SAFETY: `codec` is valid and `info` is writable.
                let output = unsafe {
                    AMediaCodec_dequeueOutputBuffer(codec, &mut info, DEQUEUE_TIMEOUT_US)
                };
                match output {
                    AMEDIACODEC_INFO_TRY_AGAIN_LATER => Ok(MediaCodecDequeueStep::Yield(
                        DecoderReceiveNativeFrameOutput::NeedMoreInput,
                    )),
                    AMEDIACODEC_INFO_OUTPUT_FORMAT_CHANGED => {
                        self.update_output_format()?;
                        Ok(MediaCodecDequeueStep::Skip)
                    }
                    AMEDIACODEC_INFO_OUTPUT_BUFFERS_CHANGED => Ok(MediaCodecDequeueStep::Skip),
                    value if value < 0 => Err(DecoderError::internal(format!(
                        "AMediaCodec_dequeueOutputBuffer returned {value}"
                    ))),
                    value => {
                        let index = usize::try_from(value).map_err(|_| {
                            DecoderError::internal("MediaCodec output index overflowed usize")
                        })?;
                        let output_kind = mediacodec_output_buffer_kind(info.flags, info.size);
                        match output_kind {
                            MediaCodecOutputBufferKind::CodecConfig => {
                                self.release_output_index(index, false)?;
                                return Ok(MediaCodecDequeueStep::Skip);
                            }
                            MediaCodecOutputBufferKind::Eof => {
                                self.release_output_index(index, false)?;
                                self.saw_output_eos = true;
                                return Ok(MediaCodecDequeueStep::Yield(
                                    DecoderReceiveNativeFrameOutput::Eof,
                                ));
                            }
                            MediaCodecOutputBufferKind::Frame { pending_eos } => {
                                if pending_eos {
                                    self.pending_output_eos = true;
                                }
                            }
                        }
                        let handle = match self.frame_lease_ids.allocate() {
                            Ok(handle) => handle,
                            Err(error) => {
                                self.release_output_index(index, false)?;
                                return Err(error);
                            }
                        };
                        if self.outstanding.contains_key(&handle) {
                            self.release_output_index(index, false)?;
                            return Err(DecoderError::abi_violation(format!(
                                "MediaCodec native-frame lease id collision for handle {handle}"
                            )));
                        }
                        self.outstanding
                            .insert(handle, MediaCodecOutputFrame { index });
                        let frame_id = u64::try_from(handle).map_err(|_| {
                            DecoderError::internal(
                                "MediaCodec frame lease id overflowed the native frame id",
                            )
                        })?;
                        let output_format = self.output_format.clone();
                        let metadata = DecoderNativeFrameMetadata {
                            media_kind: DecoderMediaKind::Video,
                            format: mediacodec_surface_texture_format(),
                            codec: codec_name.to_owned(),
                            pts_us: Some(info.presentation_time_us),
                            duration_us: None,
                            width: output_format.width,
                            height: output_format.height,
                            coded_width: Some(output_format.coded_width),
                            coded_height: Some(output_format.coded_height),
                            visible_rect: Some(output_format.visible_rect),
                            handle_kind: DecoderNativeHandleKind::MediaCodecSurfaceTexture,
                            pipeline_profile: Some(
                                NativeFramePipelineProfile::MediaCodecSurfaceTexture,
                            ),
                            color_space: None,
                            hdr_metadata: output_format.hdr.as_ref().map(|hdr| hdr.kind.clone()),
                            color: output_format.color.clone(),
                            hdr: output_format.hdr.clone(),
                            sync_info: None,
                            transform: None,
                            frame_id: Some(frame_id),
                            release_tracking: Some(DecoderNativeFrameReleaseTracking {
                                frame_id: Some(frame_id),
                                requires_release: true,
                            }),
                        };
                        Ok(MediaCodecDequeueStep::Yield(
                            DecoderReceiveNativeFrameOutput::Frame(DecoderNativeFrame {
                                metadata,
                                handle,
                                lease_token: None,
                            }),
                        ))
                    }
                }
            })?;
            Ok(match output {
                Some(output) => output,
                None => DecoderReceiveNativeFrameOutput::NeedMoreInput,
            })
        }

        pub(super) fn release_native_frame(
            &mut self,
            handle: usize,
            presented: bool,
        ) -> Result<(), DecoderError> {
            let Some(frame) = self.outstanding.get(&handle).copied() else {
                return Err(DecoderError::abi_violation(format!(
                    "unknown MediaCodec output frame handle {handle}"
                )));
            };
            let release_result = self.release_output_index(frame.index, presented);
            complete_mediacodec_frame_release(&mut self.outstanding, handle, release_result)
        }

        pub(super) fn flush(&mut self) -> Result<(), DecoderError> {
            self.release_all_outstanding(false)?;
            let codec = self.codec.as_ptr()?;
            // SAFETY: `codec` is valid and started.
            let status = unsafe { AMediaCodec_flush(codec) };
            if status != AMEDIA_OK {
                return Err(media_status_error("AMediaCodec_flush", status));
            }
            self.saw_input_eos = false;
            self.saw_output_eos = false;
            self.pending_output_eos = false;
            Ok(())
        }

        pub(super) fn close(&mut self) -> Result<(), DecoderError> {
            if self.closed {
                return Ok(());
            }
            let release_result = self.release_all_outstanding(false);
            let codec = self.codec.as_ptr()?;
            // SAFETY: `codec` is valid until delete completes.
            let stop_status = unsafe { AMediaCodec_stop(codec) };
            let delete_status = self.codec.delete();
            self.closed = true;
            media_codec_close_result(release_result, stop_status, delete_status)
        }

        fn dequeue_input(&mut self) -> Result<Option<usize>, DecoderError> {
            let codec = self.codec.as_ptr()?;
            // SAFETY: `codec` is valid and started.
            let index = unsafe { AMediaCodec_dequeueInputBuffer(codec, DEQUEUE_TIMEOUT_US) };
            match index {
                AMEDIACODEC_INFO_TRY_AGAIN_LATER => Ok(None),
                value if value < 0 => Err(DecoderError::internal(format!(
                    "AMediaCodec_dequeueInputBuffer returned {value}"
                ))),
                value => usize::try_from(value)
                    .map(Some)
                    .map_err(|_| DecoderError::internal("MediaCodec input index overflowed usize")),
            }
        }

        fn copy_input(&mut self, index: usize, data: &[u8]) -> Result<(), DecoderError> {
            let mut capacity = 0_usize;
            let codec = self.codec.as_ptr()?;
            // SAFETY: `index` came from dequeueInputBuffer and `capacity` is writable.
            let buffer = unsafe { AMediaCodec_getInputBuffer(codec, index, &mut capacity) };
            let Some(buffer) = NonNull::new(buffer) else {
                return Err(DecoderError::internal(
                    "AMediaCodec_getInputBuffer returned null",
                ));
            };
            if data.len() > capacity {
                return Err(DecoderError::InvalidPacket {
                    message: format!(
                        "MediaCodec input packet is larger than input buffer: {} > {}",
                        data.len(),
                        capacity
                    ),
                });
            }
            // SAFETY: `buffer` points to an input buffer with at least `capacity`
            // bytes and `data.len() <= capacity`.
            unsafe {
                std::ptr::copy_nonoverlapping(data.as_ptr(), buffer.as_ptr(), data.len());
            }
            Ok(())
        }

        fn update_output_format(&mut self) -> Result<(), DecoderError> {
            let codec = self.codec.as_ptr()?;
            // SAFETY: `codec` is valid and started. The returned AMediaFormat is
            // owned by the caller and released by the MediaFormat wrapper below.
            let Some(format) = NonNull::new(unsafe { AMediaCodec_getOutputFormat(codec) }) else {
                return Err(DecoderError::internal(
                    "AMediaCodec_getOutputFormat returned null",
                ));
            };
            let format = MediaFormat { format };
            self.output_format = output_format_from_media_format(&format, &self.output_format);
            Ok(())
        }

        fn release_all_outstanding(&mut self, presented: bool) -> Result<(), DecoderError> {
            let outstanding = std::mem::take(&mut self.outstanding);
            let mut first_error = None;
            for (_, frame) in outstanding {
                if let Err(error) = self.release_output_index(frame.index, presented)
                    && first_error.is_none()
                {
                    first_error = Some(error);
                }
            }
            match first_error {
                Some(error) => Err(error),
                None => Ok(()),
            }
        }

        fn release_output_index(
            &mut self,
            index: usize,
            presented: bool,
        ) -> Result<(), DecoderError> {
            let codec = self.codec.as_ptr()?;
            // SAFETY: `index` is an outstanding output buffer index from this codec.
            let status = unsafe { AMediaCodec_releaseOutputBuffer(codec, index, presented) };
            if status == AMEDIA_OK {
                Ok(())
            } else {
                Err(media_status_error(
                    "AMediaCodec_releaseOutputBuffer",
                    status,
                ))
            }
        }
    }

    impl Drop for AndroidMediaCodecBackend {
        fn drop(&mut self) {
            let _ = self.close();
        }
    }

    struct MediaFormat {
        format: NonNull<AMediaFormat>,
    }

    impl MediaFormat {
        fn new() -> Result<Self, DecoderError> {
            // SAFETY: AMediaFormat_new returns a new object or null.
            NonNull::new(unsafe { AMediaFormat_new() })
                .map(|format| Self { format })
                .ok_or_else(|| DecoderError::internal("AMediaFormat_new returned null"))
        }

        fn as_ptr(&self) -> *mut AMediaFormat {
            self.format.as_ptr()
        }

        fn get_i32(&self, key: *const c_char) -> Option<i32> {
            if key.is_null() {
                return None;
            }
            let mut value = 0;
            // SAFETY: `self.format` is a valid AMediaFormat, `key` is a
            // process-lifetime NDK key, and `value` is writable.
            if unsafe { AMediaFormat_getInt32(self.format.as_ptr(), key, &mut value) } {
                Some(value)
            } else {
                None
            }
        }
    }

    impl Drop for MediaFormat {
        fn drop(&mut self) {
            // SAFETY: `format` was created by AMediaFormat_new and is deleted once here.
            unsafe {
                let _ = AMediaFormat_delete(self.format.as_ptr());
            }
        }
    }

    fn output_format_from_media_format(
        format: &MediaFormat,
        fallback: &MediaCodecOutputFormat,
    ) -> MediaCodecOutputFormat {
        let coded_width = format
            .get_i32(unsafe { AMEDIAFORMAT_KEY_WIDTH })
            .and_then(non_negative_i32_to_u32)
            .unwrap_or(fallback.coded_width);
        let coded_height = format
            .get_i32(unsafe { AMEDIAFORMAT_KEY_HEIGHT })
            .and_then(non_negative_i32_to_u32)
            .unwrap_or(fallback.coded_height);
        let crop_left = format
            .get_i32(AMEDIAFORMAT_KEY_CROP_LEFT_FALLBACK.as_ptr())
            .and_then(non_negative_i32_to_u32);
        let crop_right = format
            .get_i32(AMEDIAFORMAT_KEY_CROP_RIGHT_FALLBACK.as_ptr())
            .and_then(non_negative_i32_to_u32);
        let crop_top = format
            .get_i32(AMEDIAFORMAT_KEY_CROP_TOP_FALLBACK.as_ptr())
            .and_then(non_negative_i32_to_u32);
        let crop_bottom = format
            .get_i32(AMEDIAFORMAT_KEY_CROP_BOTTOM_FALLBACK.as_ptr())
            .and_then(non_negative_i32_to_u32);
        let visible_rect = match (crop_left, crop_right, crop_top, crop_bottom) {
            (Some(left), Some(right), Some(top), Some(bottom))
                if right >= left && bottom >= top =>
            {
                DecoderVisibleRect {
                    x: left,
                    y: top,
                    width: right.saturating_sub(left).saturating_add(1),
                    height: bottom.saturating_sub(top).saturating_add(1),
                }
            }
            _ => DecoderVisibleRect {
                x: 0,
                y: 0,
                width: coded_width,
                height: coded_height,
            },
        };
        let mut output_format = normalize_output_format(coded_width, coded_height, visible_rect);
        output_format.color = merge_mediacodec_color_metadata(
            fallback.color.clone(),
            media_format_color_metadata(format),
        );
        output_format.hdr = merge_mediacodec_hdr_metadata(
            fallback.hdr.clone(),
            mediacodec_hdr_metadata_from_color(output_format.color.as_ref()),
        );
        output_format
    }

    fn normalize_output_format(
        coded_width: u32,
        coded_height: u32,
        visible_rect: DecoderVisibleRect,
    ) -> MediaCodecOutputFormat {
        let fallback_width = coded_width.max(1);
        let fallback_height = coded_height.max(1);
        let visible_width = visible_rect.width.min(fallback_width).max(1);
        let visible_height = visible_rect.height.min(fallback_height).max(1);
        MediaCodecOutputFormat {
            width: visible_width,
            height: visible_height,
            coded_width: fallback_width,
            coded_height: fallback_height,
            visible_rect,
            color: None,
            hdr: None,
        }
    }

    fn media_format_color_metadata(format: &MediaFormat) -> Option<NativeFrameColorMetadata> {
        let color = NativeFrameColorMetadata {
            primaries: format
                .get_i32(AMEDIAFORMAT_KEY_COLOR_STANDARD_FALLBACK.as_ptr())
                .and_then(android_color_standard_label),
            transfer: format
                .get_i32(AMEDIAFORMAT_KEY_COLOR_TRANSFER_FALLBACK.as_ptr())
                .and_then(android_color_transfer_label),
            matrix: format
                .get_i32(AMEDIAFORMAT_KEY_COLOR_STANDARD_FALLBACK.as_ptr())
                .and_then(android_color_standard_matrix_label),
            range: format
                .get_i32(AMEDIAFORMAT_KEY_COLOR_RANGE_FALLBACK.as_ptr())
                .and_then(android_color_range_label),
            bit_depth: None,
        };
        if color.primaries.is_none()
            && color.transfer.is_none()
            && color.matrix.is_none()
            && color.range.is_none()
        {
            None
        } else {
            Some(color)
        }
    }

    fn non_negative_i32_to_u32(value: i32) -> Option<u32> {
        u32::try_from(value).ok()
    }

    fn media_status_error(operation: &str, status: MediaStatus) -> DecoderError {
        DecoderError::internal(format!("{operation} failed with media_status_t={status}"))
    }
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn media_codec_close_result(
    release_result: Result<(), DecoderError>,
    stop_status: MediaStatus,
    delete_status: MediaStatus,
) -> Result<(), DecoderError> {
    release_result?;
    if stop_status != AMEDIA_OK {
        return Err(media_status_error("AMediaCodec_stop", stop_status));
    }
    if delete_status != AMEDIA_OK {
        tracing::warn!(
            media_status = delete_status,
            "AMediaCodec_delete failed after destructive MediaCodec teardown"
        );
    }
    Ok(())
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn media_status_error(operation: &str, status: MediaStatus) -> DecoderError {
    DecoderError::internal(format!("{operation} failed with media_status_t={status}"))
}

#[cfg(test)]
mod tests {
    use super::{
        AMEDIA_OK, MAX_OUTPUT_DEQUEUE_STEPS_PER_CALL, MediaCodecDecoderSession,
        MediaCodecDequeueStep, MediaCodecFrameLeaseIds, MediaCodecOutputBufferKind,
        MediaCodecPendingOutputEosAction, android_color_standard_label,
        android_color_standard_matrix_label, android_color_transfer_label,
        android_native_window_ptr, codec_config_buffers, codec_mime,
        complete_mediacodec_frame_release, decoder_capabilities, decoder_native_requirements,
        length_prefixed_sample_to_annex_b, media_codec_close_result,
        mediacodec_hdr_metadata_from_color, mediacodec_output_buffer_kind,
        mediacodec_pending_output_eos_action, mediacodec_surface_texture_format,
        merge_mediacodec_color_metadata, merge_mediacodec_hdr_metadata, nal_length_size_for_config,
        packet_data_for_mediacodec, packet_input_data_for_mediacodec,
        required_mediacodec_decoder_implementation_name, run_bounded_mediacodec_dequeue,
        split_avcc_extradata, split_hvcc_extradata, vesper_plugin_entry,
    };
    use player_plugin::{
        DecoderBitstreamFormat, DecoderError, DecoderFrameFormat, DecoderMediaKind,
        DecoderNativeDeviceContext, DecoderNativeDeviceContextKind, DecoderNativeHandleKind,
        DecoderPacket, DecoderSessionConfig, NativeFrameColorMetadata,
        NativeFrameContentLightMetadata, NativeFrameDolbyVisionMetadata, NativeFrameHdrMetadata,
        NativeFrameMasteringDisplayMetadata, NativeFramePipelineProfile,
    };
    use std::collections::HashMap;

    #[test]
    fn mediacodec_close_ignores_delete_failure_after_destructive_teardown() {
        media_codec_close_result(Ok(()), AMEDIA_OK, -100)
            .expect("delete failure after teardown is not retryable");
    }

    #[test]
    fn mediacodec_close_reports_stop_failure() {
        let error = media_codec_close_result(Ok(()), -22, AMEDIA_OK)
            .expect_err("stop failure should remain visible");

        assert!(error.to_string().contains("AMediaCodec_stop"));
    }

    #[test]
    fn mediacodec_close_reports_release_failure() {
        let error = media_codec_close_result(
            Err(DecoderError::internal("release outstanding failed")),
            AMEDIA_OK,
            AMEDIA_OK,
        )
        .expect_err("release failure should remain visible");

        assert!(error.to_string().contains("release outstanding failed"));
    }

    #[test]
    fn exports_plugin_entry() {
        let entry: extern "C" fn() -> *const player_plugin::__private::VesperPluginRoot =
            vesper_plugin_entry;
        assert!(!entry().is_null());
    }

    #[test]
    fn decoder_implementation_name_is_required_and_preserved_exactly() {
        let mut config = DecoderSessionConfig::default();
        let missing = required_mediacodec_decoder_implementation_name(&config)
            .expect_err("MediaCodec requires an exact host-selected decoder name");
        assert!(matches!(
            missing,
            DecoderError::UnsupportedCapability { capability }
                if capability == "host-selected-hardware-decoder-implementation"
        ));

        config.required_decoder_implementation_name = Some(" \t ".to_owned());
        assert!(required_mediacodec_decoder_implementation_name(&config).is_err());

        config.required_decoder_implementation_name = Some("c2.vendor.avc.decoder".to_owned());
        assert_eq!(
            required_mediacodec_decoder_implementation_name(&config),
            Ok("c2.vendor.avc.decoder")
        );

        config.required_decoder_implementation_name = Some(" c2.vendor.avc.decoder ".to_owned());
        assert_eq!(
            required_mediacodec_decoder_implementation_name(&config),
            Ok(" c2.vendor.avc.decoder ")
        );
    }

    #[test]
    fn bounded_dequeue_stops_after_sixteen_skippable_events() {
        let mut calls = 0_usize;
        let output = run_bounded_mediacodec_dequeue(|| {
            calls += 1;
            Ok::<_, DecoderError>(MediaCodecDequeueStep::<&str>::Skip)
        })
        .expect("skippable dequeue events do not fail");

        assert_eq!(output, None);
        assert_eq!(calls, MAX_OUTPUT_DEQUEUE_STEPS_PER_CALL);
    }

    #[test]
    fn bounded_dequeue_returns_frame_after_fifteen_skippable_events() {
        let mut calls = 0_usize;
        let output = run_bounded_mediacodec_dequeue(|| {
            calls += 1;
            if calls < MAX_OUTPUT_DEQUEUE_STEPS_PER_CALL {
                Ok::<_, DecoderError>(MediaCodecDequeueStep::Skip)
            } else {
                Ok(MediaCodecDequeueStep::Yield("frame"))
            }
        })
        .expect("the sixteenth dequeue step can yield a frame");

        assert_eq!(output, Some("frame"));
        assert_eq!(calls, MAX_OUTPUT_DEQUEUE_STEPS_PER_CALL);
    }

    #[test]
    fn bounded_dequeue_returns_immediate_try_again_result() {
        let mut calls = 0_usize;
        let output = run_bounded_mediacodec_dequeue(|| {
            calls += 1;
            Ok::<_, DecoderError>(MediaCodecDequeueStep::Yield("need-more-input"))
        })
        .expect("try-again is a successful dequeue outcome");

        assert_eq!(output, Some("need-more-input"));
        assert_eq!(calls, 1);
    }

    #[test]
    fn capabilities_advertise_android_mediacodec_video_contract() {
        let capabilities = decoder_capabilities();

        assert_eq!(
            capabilities.supports_codec("h264", DecoderMediaKind::Video),
            cfg!(target_os = "android")
        );
        assert_eq!(
            capabilities.supports_codec("HEV1", DecoderMediaKind::Video),
            cfg!(target_os = "android")
        );
        assert!(!capabilities.supports_codec("dvh1", DecoderMediaKind::Video));
        assert!(!capabilities.supports_codec("dvhe", DecoderMediaKind::Video));
        assert!(!capabilities.supports_audio_frames);
        assert!(!capabilities.supports_cpu_video_frames);
        assert_eq!(
            capabilities.supports_hardware_decode,
            cfg!(target_os = "android")
        );
        assert_eq!(
            capabilities.supports_gpu_handles,
            cfg!(target_os = "android")
        );
        for capability in &capabilities.codecs {
            assert_eq!(
                capability.output_formats,
                vec![DecoderFrameFormat::Unknown(
                    "mediacodec_surface_texture".to_owned()
                )]
            );
        }
    }

    #[test]
    fn android_color_labels_match_media_format_constants() {
        assert_eq!(android_color_standard_label(1).as_deref(), Some("bt709"));
        assert_eq!(
            android_color_standard_label(2).as_deref(),
            Some("bt601-pal")
        );
        assert_eq!(
            android_color_standard_label(4).as_deref(),
            Some("bt601-ntsc")
        );
        assert_eq!(android_color_standard_label(6).as_deref(), Some("bt2020"));
        assert_eq!(
            android_color_standard_matrix_label(2).as_deref(),
            Some("bt601")
        );
        assert_eq!(
            android_color_standard_matrix_label(4).as_deref(),
            Some("bt601")
        );
        assert_eq!(
            android_color_standard_matrix_label(6).as_deref(),
            Some("bt2020-ncl")
        );
        assert_eq!(
            android_color_transfer_label(3).as_deref(),
            Some("sdr-video")
        );
        assert_eq!(android_color_transfer_label(6).as_deref(), Some("st2084"));
        assert_eq!(android_color_transfer_label(7).as_deref(), Some("hlg"));
    }

    #[test]
    fn android_hdr_metadata_uses_correct_transfer_constants() {
        let sdr = NativeFrameColorMetadata {
            primaries: Some("bt709".to_owned()),
            transfer: android_color_transfer_label(3),
            matrix: Some("bt709".to_owned()),
            range: Some("limited".to_owned()),
            bit_depth: Some(8),
        };
        assert!(mediacodec_hdr_metadata_from_color(Some(&sdr)).is_none());

        let hdr10 = NativeFrameColorMetadata {
            transfer: android_color_transfer_label(6),
            ..sdr.clone()
        };
        assert_eq!(
            mediacodec_hdr_metadata_from_color(Some(&hdr10))
                .as_ref()
                .map(|hdr| hdr.kind.as_str()),
            Some("hdr10")
        );

        let hlg = NativeFrameColorMetadata {
            transfer: android_color_transfer_label(7),
            ..sdr
        };
        assert_eq!(
            mediacodec_hdr_metadata_from_color(Some(&hlg))
                .as_ref()
                .map(|hdr| hdr.kind.as_str()),
            Some("hlg")
        );
    }

    #[test]
    fn color_metadata_merge_preserves_config_fields_missing_from_runtime() {
        let fallback = NativeFrameColorMetadata {
            primaries: Some("bt2020".to_owned()),
            transfer: Some("st2084".to_owned()),
            matrix: Some("bt2020-ncl".to_owned()),
            range: Some("limited".to_owned()),
            bit_depth: Some(10),
        };
        let runtime = NativeFrameColorMetadata {
            primaries: None,
            transfer: None,
            matrix: None,
            range: Some("full".to_owned()),
            bit_depth: None,
        };

        let merged = merge_mediacodec_color_metadata(Some(fallback), Some(runtime))
            .expect("runtime range should merge with fallback color");

        assert_eq!(merged.primaries.as_deref(), Some("bt2020"));
        assert_eq!(merged.transfer.as_deref(), Some("st2084"));
        assert_eq!(merged.matrix.as_deref(), Some("bt2020-ncl"));
        assert_eq!(merged.range.as_deref(), Some("full"));
        assert_eq!(merged.bit_depth, Some(10));
    }

    #[test]
    fn hdr_metadata_merge_does_not_downgrade_dolby_vision() {
        let fallback = NativeFrameHdrMetadata {
            kind: "dolbyVision".to_owned(),
            mastering_display: None,
            content_light: None,
            dolby_vision: Some(NativeFrameDolbyVisionMetadata {
                profile: Some(8),
                level: Some(6),
                compatibility_id: Some(1),
                has_rpu: true,
                has_el: false,
                has_bl: true,
            }),
        };
        let runtime = NativeFrameHdrMetadata {
            kind: "hdr10".to_owned(),
            mastering_display: None,
            content_light: None,
            dolby_vision: None,
        };

        let merged = merge_mediacodec_hdr_metadata(Some(fallback), Some(runtime))
            .expect("Dolby Vision metadata should be preserved");

        assert_eq!(merged.kind, "dolbyVision");
        assert!(merged.dolby_vision.is_some());
    }

    #[test]
    fn hdr_metadata_merge_preserves_static_hdr_metadata() {
        let fallback = NativeFrameHdrMetadata {
            kind: "hdr10".to_owned(),
            mastering_display: Some(NativeFrameMasteringDisplayMetadata {
                display_primaries: Some("bt2020".to_owned()),
                white_point: Some("d65".to_owned()),
                max_luminance_nits: Some(1_000),
                min_luminance_nits: Some(0),
            }),
            content_light: Some(NativeFrameContentLightMetadata {
                max_content_light_level: Some(1_000),
                max_frame_average_light_level: Some(400),
            }),
            dolby_vision: None,
        };
        let runtime = NativeFrameHdrMetadata {
            kind: "hdr10".to_owned(),
            mastering_display: None,
            content_light: None,
            dolby_vision: None,
        };

        let merged = merge_mediacodec_hdr_metadata(Some(fallback), Some(runtime))
            .expect("HDR10 static metadata should be preserved");

        assert_eq!(merged.kind, "hdr10");
        assert!(merged.mastering_display.is_some());
        assert!(merged.content_light.is_some());
    }

    #[test]
    fn session_info_reports_opaque_surface_texture_format() {
        assert_eq!(
            mediacodec_surface_texture_format(),
            DecoderFrameFormat::Unknown("mediacodec_surface_texture".to_owned())
        );
    }

    #[test]
    fn native_requirements_need_android_native_window_and_surface_output() {
        let requirements = decoder_native_requirements();

        assert_eq!(
            requirements.required_device_context_kinds,
            vec![DecoderNativeDeviceContextKind::AndroidNativeWindow]
        );
        assert_eq!(
            requirements.output_handle_kinds,
            vec![DecoderNativeHandleKind::MediaCodecSurfaceTexture]
        );
        assert_eq!(
            requirements.output_pipeline_profiles,
            vec![NativeFramePipelineProfile::MediaCodecSurfaceTexture]
        );
        assert!(requirements.requires_native_device_context);
        assert_eq!(
            requirements.accepted_bitstream_formats,
            vec![
                DecoderBitstreamFormat::AnnexB,
                DecoderBitstreamFormat::Avcc,
                DecoderBitstreamFormat::Hvcc,
            ]
        );
    }

    #[test]
    fn native_requirements_do_not_advertise_hardware_buffer_before_runtime_support() {
        let requirements = decoder_native_requirements();

        assert!(
            !requirements
                .output_handle_kinds
                .contains(&DecoderNativeHandleKind::MediaCodecHardwareBuffer)
        );
        assert!(
            !requirements
                .output_pipeline_profiles
                .contains(&NativeFramePipelineProfile::MediaCodecHardwareBuffer)
        );
    }

    #[test]
    fn android_native_window_context_rejects_missing_or_wrong_context() {
        assert!(matches!(
            android_native_window_ptr(None),
            Err(DecoderError::UnsupportedCapability { .. })
        ));
        assert!(matches!(
            android_native_window_ptr(Some(&DecoderNativeDeviceContext::D3D11Device {
                device_ptr: 7,
            })),
            Err(DecoderError::UnsupportedCapability { .. })
        ));
        assert!(matches!(
            android_native_window_ptr(Some(&DecoderNativeDeviceContext::AndroidNativeWindow {
                window_ptr: 0,
            })),
            Err(DecoderError::AbiViolation { .. })
        ));
        assert_eq!(
            android_native_window_ptr(Some(&DecoderNativeDeviceContext::AndroidNativeWindow {
                window_ptr: 0x1234,
            })),
            Ok(0x1234)
        );
    }

    #[test]
    fn codec_mime_maps_supported_android_video_codecs() {
        assert_eq!(codec_mime("h264"), Some("video/avc"));
        assert_eq!(codec_mime("AVC1"), Some("video/avc"));
        assert_eq!(codec_mime("AVC3"), Some("video/avc"));
        assert_eq!(codec_mime("hevc"), Some("video/hevc"));
        assert_eq!(codec_mime("HVC1"), Some("video/hevc"));
        assert_eq!(codec_mime("dvh1.05.06"), None);
        assert_eq!(codec_mime("dvhe.08.07"), None);
        assert_eq!(codec_mime("vp9"), None);
    }

    #[test]
    fn avcc_extradata_splits_sps_and_pps_as_annex_b_csd_buffers() {
        let avcc = vec![
            1, 0x64, 0, 0x1f, 0xff, 0xe1, 0, 4, 0x67, 0x64, 0, 0x1f, 1, 0, 2, 0x68, 0xeb,
        ];

        assert_eq!(
            split_avcc_extradata(&avcc),
            vec![
                vec![0, 0, 0, 1, 0x67, 0x64, 0, 0x1f],
                vec![0, 0, 0, 1, 0x68, 0xeb],
            ]
        );
    }

    #[test]
    fn hvcc_extradata_splits_parameter_arrays_as_annex_b_csd_buffers() {
        let hvcc = vec![
            1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0xa1, 0, 1, 0, 3,
            0x40, 0x01, 0x0c,
        ];

        assert_eq!(
            split_hvcc_extradata(&hvcc),
            vec![vec![0, 0, 0, 1, 0x40, 0x01, 0x0c]]
        );
    }

    #[test]
    fn codec_config_buffers_fall_back_to_raw_extradata_for_unknown_format() {
        let config = DecoderSessionConfig {
            codec: "h264".to_owned(),
            media_kind: DecoderMediaKind::Video,
            extradata: vec![1, 2, 3],
            bitstream_format: Some(DecoderBitstreamFormat::Unknown("fixture".to_owned())),
            ..DecoderSessionConfig::default()
        };

        assert_eq!(codec_config_buffers(&config), vec![vec![1, 2, 3]]);
    }

    #[test]
    fn mediacodec_packet_data_converts_avcc_samples_to_annex_b() {
        let config = DecoderSessionConfig {
            codec: "h264".to_owned(),
            media_kind: DecoderMediaKind::Video,
            extradata: vec![1, 0x64, 0, 0x1f, 0xff],
            bitstream_format: Some(DecoderBitstreamFormat::Avcc),
            ..DecoderSessionConfig::default()
        };
        let sample = vec![0, 0, 0, 3, 0x65, 0x88, 0x84, 0, 0, 0, 2, 0x41, 0x9a];

        let converted = packet_data_for_mediacodec(
            config.bitstream_format.as_ref(),
            nal_length_size_for_config(&config),
            &sample,
        )
        .expect("AVCC sample converts");

        assert_eq!(
            converted.as_ref(),
            &[0, 0, 0, 1, 0x65, 0x88, 0x84, 0, 0, 0, 1, 0x41, 0x9a]
        );
    }

    #[test]
    fn mediacodec_packet_data_keeps_existing_annex_b_samples_borrowed() {
        let sample = vec![0, 0, 0, 1, 0x65, 0x88, 0x84];
        let converted = packet_data_for_mediacodec(Some(&DecoderBitstreamFormat::Avcc), 4, &sample)
            .expect("Annex B sample is accepted");

        assert!(matches!(converted, std::borrow::Cow::Borrowed(_)));
        assert_eq!(converted.as_ref(), sample.as_slice());
    }

    #[test]
    fn mediacodec_packet_data_honors_hvcc_nal_length_size() {
        let mut extradata = vec![0; 22];
        extradata[0] = 1;
        extradata[21] = 0x01;
        let config = DecoderSessionConfig {
            codec: "hevc".to_owned(),
            media_kind: DecoderMediaKind::Video,
            extradata,
            bitstream_format: Some(DecoderBitstreamFormat::Hvcc),
            ..DecoderSessionConfig::default()
        };
        let sample = vec![0, 3, 0x26, 0x01, 0xaf];

        let converted = packet_data_for_mediacodec(
            config.bitstream_format.as_ref(),
            nal_length_size_for_config(&config),
            &sample,
        )
        .expect("HVCC sample converts");

        assert_eq!(converted.as_ref(), &[0, 0, 0, 1, 0x26, 0x01, 0xaf]);
    }

    #[test]
    fn mediacodec_eos_packet_queues_empty_input_data() {
        let packet = DecoderPacket {
            end_of_stream: true,
            ..DecoderPacket::default()
        };

        let input = packet_input_data_for_mediacodec(
            &packet,
            Some(&DecoderBitstreamFormat::Avcc),
            4,
            &[0, 0, 0, 1, 0x65],
        )
        .expect("EOS packet input");

        assert!(input.is_empty());
    }

    #[test]
    fn mediacodec_output_buffer_kind_keeps_final_frame_with_eos_flag() {
        assert_eq!(
            mediacodec_output_buffer_kind(4, 128),
            MediaCodecOutputBufferKind::Frame { pending_eos: true }
        );
        assert_eq!(
            mediacodec_output_buffer_kind(4, 0),
            MediaCodecOutputBufferKind::Eof
        );
        assert_eq!(
            mediacodec_output_buffer_kind(2, 0),
            MediaCodecOutputBufferKind::CodecConfig
        );
    }

    #[test]
    fn mediacodec_pending_output_eos_waits_for_last_frame_release() {
        assert_eq!(
            mediacodec_pending_output_eos_action(1),
            MediaCodecPendingOutputEosAction::WaitForOutstandingRelease
        );
        assert_eq!(
            mediacodec_pending_output_eos_action(0),
            MediaCodecPendingOutputEosAction::ReportEof
        );
    }

    #[test]
    fn mediacodec_frame_lease_ids_do_not_reuse_codec_output_indices() {
        let mut lease_ids = MediaCodecFrameLeaseIds::default();

        let first = lease_ids.allocate().expect("first frame lease id");
        let second = lease_ids.allocate().expect("second frame lease id");

        assert_eq!(first, 1);
        assert_eq!(second, 2);
        assert_ne!(first, second);
    }

    #[test]
    fn mediacodec_frame_lease_id_exhaustion_is_typed() {
        let mut lease_ids = MediaCodecFrameLeaseIds { next: usize::MAX };

        let error = lease_ids
            .allocate()
            .expect_err("lease id overflow must not wrap to a stale identity");

        assert!(matches!(error, DecoderError::Internal { .. }));
    }

    #[test]
    fn mediacodec_release_failure_keeps_frame_lease_retryable() {
        let mut outstanding = HashMap::from([(7_usize, 3_usize)]);

        let error = complete_mediacodec_frame_release(
            &mut outstanding,
            7,
            Err(DecoderError::internal("release output buffer failed")),
        )
        .expect_err("the platform release failure must remain visible");

        assert!(error.to_string().contains("release output buffer failed"));
        assert_eq!(outstanding.get(&7), Some(&3));

        complete_mediacodec_frame_release(&mut outstanding, 7, Ok(()))
            .expect("retry should retire the lease after release succeeds");
        assert!(!outstanding.contains_key(&7));
    }

    #[test]
    fn length_prefixed_sample_rejects_truncated_nal() {
        let error = length_prefixed_sample_to_annex_b(&[0, 0, 0, 4, 0x65], 4)
            .expect_err("truncated sample is invalid");

        assert!(matches!(error, DecoderError::InvalidPacket { .. }));
    }

    #[test]
    #[cfg(not(target_os = "android"))]
    fn safe_session_reports_unsupported_outside_android() {
        let config = DecoderSessionConfig {
            codec: "h264".to_owned(),
            media_kind: DecoderMediaKind::Video,
            native_device_context: Some(DecoderNativeDeviceContext::AndroidNativeWindow {
                window_ptr: 0x1234,
            }),
            ..DecoderSessionConfig::default()
        };
        let error = MediaCodecDecoderSession::open(config)
            .expect_err("MediaCodec is unavailable outside Android");
        assert!(matches!(
            error,
            DecoderError::UnsupportedCapability { capability }
                if capability == "android-mediacodec-decoder"
        ));
    }
}
