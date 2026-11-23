use super::*;
use player_plugin::{
    DecoderCapabilities, DecoderError, DecoderFrameFormat, DecoderMediaKind,
    DecoderNativeDeviceContextKind, DecoderNativeFrame, DecoderNativeFrameMetadata,
    DecoderNativeFrameReleaseTracking, DecoderNativeHandleKind, DecoderNativeRequirements,
    DecoderPacket, DecoderPacketResult, DecoderReceiveNativeFrameOutput, DecoderSessionConfig,
    DecoderSessionInfo, DecoderSessionRequirements, DecoderVisibleRect, NativeDecoderPluginFactory,
    NativeDecoderSession, NativeFrameColorMetadata, NativeFrameLeaseToken,
    NativeFramePipelineProfile, normalize_decoder_codec_identifier,
};
use std::sync::atomic::{AtomicU64, Ordering};

const MAX_CAPABILITY_BYTES: usize = 16 * 1024;
const MAX_IN_FLIGHT_FRAMES: usize = 64;
const MAX_FORMAT_CHANGES_PER_POLL: usize = 8;
const CALL_BUDGET: Duration = Duration::from_millis(300);
static NEXT_SESSION: AtomicU64 = AtomicU64::new(1);

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Capabilities {
    decoder: DecoderCapabilities,
    native_requirements: DecoderNativeRequirements,
}

struct Factory {
    plugin: NativePlugin,
    capabilities: Capabilities,
}

impl NativePlugin {
    pub(crate) fn native_decoder_factory(
        &self,
    ) -> PluginResult<Arc<dyn NativeDecoderPluginFactory>> {
        let query =
            self.video_decoder_table()?
                .query_capabilities_json
                .ok_or(PluginError::Contract(
                    "video decoder does not declare host admission capabilities",
                ))?;
        let mut json = VesperBytes::default();
        // SAFETY: the root remains live and json is a writable output descriptor.
        check_status(unsafe { query(self.root.0.owner, &mut json) })?;
        // SAFETY: the capability response is borrowed from the immutable root.
        let json = unsafe { read_bytes(json, MAX_CAPABILITY_BYTES) }?;
        let mut capabilities: Capabilities = serde_json::from_slice(json)
            .map_err(|_| PluginError::Contract("invalid video decoder capabilities"))?;
        if capabilities
            .native_requirements
            .output_handle_kinds
            .contains(&DecoderNativeHandleKind::CvPixelBuffer)
        {
            capabilities.decoder.supports_presentation_release = true;
        }
        Ok(Arc::new(Factory {
            plugin: self.clone(),
            capabilities,
        }))
    }
}

impl NativeDecoderPluginFactory for Factory {
    fn name(&self) -> &str {
        self.plugin.name()
    }

    fn capabilities(&self) -> DecoderCapabilities {
        self.capabilities.decoder.clone()
    }

    fn native_requirements(&self) -> DecoderNativeRequirements {
        self.capabilities.native_requirements.clone()
    }

    fn open_native_session(
        &self,
        config: &DecoderSessionConfig,
    ) -> Result<Box<dyn NativeDecoderSession>, DecoderError> {
        let declared = &self.capabilities.native_requirements;
        let (handle_kind, profile, backend, gpu) = match config.native_device_context.as_ref() {
            None if !declared.requires_native_device_context => (
                DecoderNativeHandleKind::CvPixelBuffer,
                NativeFramePipelineProfile::VideoToolboxCvPixelBuffer,
                "videotoolbox",
                None,
            ),
            Some(context) if cfg!(windows) => {
                let device = context
                    .d3d11_device_ptr()
                    .filter(|ptr| *ptr != 0)
                    .ok_or_else(|| unsupported("native D3D11 device"))?;
                if !declared
                    .required_device_context_kinds
                    .contains(&DecoderNativeDeviceContextKind::D3D11Device)
                {
                    return Err(unsupported("native D3D11 decoder device admission"));
                }
                (
                    DecoderNativeHandleKind::D3D11Texture2D,
                    NativeFramePipelineProfile::D3D11Texture2D,
                    "d3d11",
                    Some(VesperGpuContext {
                        struct_size: size_of::<VesperGpuContext>() as u32,
                        backend: VESPER_GPU_D3D11,
                        data: VesperGpuContextData {
                            d3d11: VesperD3D11Context {
                                device: device as *mut c_void,
                                immediate_context: ptr::null_mut(),
                            },
                        },
                    }),
                )
            }
            _ => return Err(unsupported("native video decoder device context")),
        };
        let requirements = DecoderSessionRequirements::native_video(
            config.codec.clone(),
            handle_kind.clone(),
            profile.clone(),
        );
        if config.media_kind != DecoderMediaKind::Video
            || config.require_cpu_output
            || !requirements
                .missing_capabilities(&self.capabilities.decoder, declared)
                .is_empty()
        {
            return Err(unsupported(format!("native {backend} native video output")));
        }
        let fourcc = match normalize_decoder_codec_identifier(&config.codec).as_str() {
            "h264" | "avc" | "avc1" => *b"avc1",
            "avc3" => *b"avc3",
            "hevc" | "h265" | "hvc1" => *b"hvc1",
            "hev1" => *b"hev1",
            _ => {
                return Err(DecoderError::UnsupportedCodec {
                    codec: config.codec.clone(),
                });
            }
        };
        let params = serde_json::to_vec(&serde_json::json!({
            "reorderDepth": config.reorder_depth,
            "width": config.width,
            "height": config.height,
            "codedWidth": config.coded_width,
            "codedHeight": config.coded_height,
            "bitstreamFormat": config.bitstream_format,
        }))
        .map_err(|error| DecoderError::internal(error.to_string()))?;
        let session_id = NEXT_SESSION
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                value.checked_add(1)
            })
            .map_err(|_| DecoderError::internal("video decoder session identifiers exhausted"))?;
        let decoder = NativeVideoDecoder::open(
            Arc::clone(&self.plugin.root),
            self.plugin.video_decoder_table().map_err(map_error)?,
            gpu.as_ref(),
            u32::from_be_bytes(fourcc),
            &config.extradata,
            &params,
            CALL_BUDGET,
        )
        .map_err(map_error)?;
        Ok(Box::new(Session {
            decoder: Some(decoder),
            config: config.clone(),
            handle_kind,
            profile,
            info: DecoderSessionInfo {
                decoder_name: Some(self.name().to_owned()),
                selected_hardware_backend: Some(backend.to_owned()),
                output_format: None,
            },
            format: None,
            session_id,
            next_lease: 1,
            leases: BTreeMap::new(),
        }))
    }
}

struct Session {
    leases: BTreeMap<u64, NativeVideoFrame>,
    decoder: Option<NativeVideoDecoder>,
    config: DecoderSessionConfig,
    handle_kind: DecoderNativeHandleKind,
    profile: NativeFramePipelineProfile,
    info: DecoderSessionInfo,
    format: Option<VesperVideoFormat>,
    session_id: u64,
    next_lease: u64,
}

impl Session {
    fn decoder(&mut self) -> Result<&mut NativeVideoDecoder, DecoderError> {
        self.decoder.as_mut().ok_or(DecoderError::NotConfigured)
    }

    fn project_frame(
        &mut self,
        frame: NativeVideoFrame,
    ) -> Result<DecoderNativeFrame, DecoderError> {
        let raw = frame.descriptor();
        let d3d11_subresource_index = match (&self.handle_kind, raw.handle_kind) {
            (DecoderNativeHandleKind::CvPixelBuffer, VESPER_HANDLE_CV_PIXEL_BUFFER) => None,
            (DecoderNativeHandleKind::D3D11Texture2D, VESPER_HANDLE_D3D11_TEXTURE2D) => Some(
                u32::try_from(raw.handles[1])
                    .map_err(|_| DecoderError::abi_violation("D3D11 subresource exceeds u32"))?,
            ),
            _ => return Err(unsupported("native decoder frame handle")),
        };
        if raw.sync_kind != VESPER_SYNC_NONE {
            return Err(unsupported("native decoder frame synchronization"));
        }
        let format = self.format.ok_or(DecoderError::NotConfigured)?;
        let handle = usize::try_from(raw.handles[0])
            .map_err(|_| DecoderError::internal("video decoder returned an out-of-range handle"))?;
        let lease = self.next_lease;
        self.next_lease = lease
            .checked_add(1)
            .ok_or_else(|| DecoderError::internal("video decoder lease identifiers exhausted"))?;
        let mut color = self
            .config
            .color
            .clone()
            .unwrap_or(NativeFrameColorMetadata {
                primaries: None,
                transfer: None,
                matrix: None,
                range: None,
                bit_depth: None,
            });
        merge_abi_color(&mut color, format.color);
        let result = DecoderNativeFrame {
            handle,
            lease_token: Some(NativeFrameLeaseToken::from_host_lease(
                2,
                self.session_id,
                lease,
            )),
            metadata: DecoderNativeFrameMetadata {
                media_kind: DecoderMediaKind::Video,
                format: frame_format(format.pixel_format)?,
                codec: self.config.codec.clone(),
                pts_us: (raw.pts_us != i64::MIN).then_some(raw.pts_us),
                duration_us: (raw.duration_us != i64::MIN).then_some(raw.duration_us),
                width: format.width,
                height: format.height,
                coded_width: (format.coded_width > 0).then_some(format.coded_width),
                coded_height: (format.coded_height > 0).then_some(format.coded_height),
                visible_rect: (format.crop_w > 0 && format.crop_h > 0).then_some(
                    DecoderVisibleRect {
                        x: format.crop_x,
                        y: format.crop_y,
                        width: format.crop_w,
                        height: format.crop_h,
                    },
                ),
                handle_kind: self.handle_kind.clone(),
                d3d11_subresource_index,
                pipeline_profile: Some(self.profile.clone()),
                color_space: None,
                hdr_metadata: None,
                color: Some(color),
                hdr: self.config.hdr.clone(),
                sync_info: None,
                transform: None,
                frame_id: Some(lease),
                release_tracking: Some(DecoderNativeFrameReleaseTracking {
                    frame_id: Some(lease),
                    requires_release: true,
                }),
            },
        };
        self.leases.insert(lease, frame);
        Ok(result)
    }
}

impl NativeDecoderSession for Session {
    fn session_info(&self) -> DecoderSessionInfo {
        self.info.clone()
    }

    fn send_packet(
        &mut self,
        packet: &DecoderPacket,
        data: &[u8],
    ) -> Result<DecoderPacketResult, DecoderError> {
        if packet.media_kind != DecoderMediaKind::Video {
            return Err(DecoderError::InvalidPacket {
                message: "expected a video packet".to_owned(),
            });
        }
        let flags = (u32::from(packet.key_frame) * VESPER_PACKET_KEYFRAME)
            | (u32::from(packet.discontinuity) * VESPER_PACKET_DISCONTINUITY)
            | (u32::from(packet.end_of_stream) * VESPER_PACKET_EOS);
        let accepted = self
            .decoder()?
            .send_packet(
                VesperPacket {
                    flags,
                    pts_us: packet.pts_us.unwrap_or(i64::MIN),
                    dts_us: packet.dts_us.unwrap_or(i64::MIN),
                    duration_us: packet.duration_us.unwrap_or(i64::MIN),
                    stream_index: packet.stream_index,
                    ..Default::default()
                },
                data,
            )
            .map_err(map_error)?;
        Ok(DecoderPacketResult { accepted })
    }

    fn receive_native_frame(&mut self) -> Result<DecoderReceiveNativeFrameOutput, DecoderError> {
        use DecoderReceiveNativeFrameOutput as Output;
        if self.leases.len() >= MAX_IN_FLIGHT_FRAMES {
            return Ok(Output::NeedMoreInput);
        }
        for _ in 0..MAX_FORMAT_CHANGES_PER_POLL {
            match self.decoder()?.receive().map_err(map_error)? {
                VideoDecoderOutput::FormatChanged(format) => {
                    self.info.output_format = Some(frame_format(format.pixel_format)?);
                    self.format = Some(format);
                }
                VideoDecoderOutput::Frame(frame) => {
                    return self.project_frame(frame).map(Output::Frame);
                }
                VideoDecoderOutput::TryAgain | VideoDecoderOutput::NeedMoreInput => {
                    return Ok(Output::NeedMoreInput);
                }
                VideoDecoderOutput::EndOfStream => return Ok(Output::Eof),
            }
        }
        Err(DecoderError::AbiViolation {
            message: "video decoder exceeded the per-poll format change limit".to_owned(),
        })
    }

    fn release_native_frame(&mut self, frame: DecoderNativeFrame) -> Result<(), DecoderError> {
        self.release_native_frame_with_presentation(frame, false)
    }

    fn release_native_frame_with_presentation(
        &mut self,
        frame: DecoderNativeFrame,
        presented: bool,
    ) -> Result<(), DecoderError> {
        if let Some((2, session, lease)) = frame
            .lease_token
            .map(NativeFrameLeaseToken::host_lease_parts)
            && session == self.session_id
            && let Some(lease) = self.leases.remove(&lease)
        {
            lease.complete(presented);
            return Ok(());
        }
        Err(DecoderError::AbiViolation {
            message: "stale or foreign video decoder lease".to_owned(),
        })
    }

    fn flush(&mut self) -> Result<(), DecoderError> {
        self.decoder()?.flush().map_err(map_error)
    }

    fn close(&mut self) -> Result<(), DecoderError> {
        self.leases.clear();
        self.decoder.take();
        Ok(())
    }
}

fn frame_format(pixel_format: u32) -> Result<DecoderFrameFormat, DecoderError> {
    match pixel_format {
        VESPER_PIXEL_NV12 => Ok(DecoderFrameFormat::Nv12),
        VESPER_PIXEL_P010 => Ok(DecoderFrameFormat::P010),
        value => Err(unsupported(format!("native video pixel format {value}"))),
    }
}

fn unsupported(capability: impl Into<String>) -> DecoderError {
    DecoderError::UnsupportedCapability {
        capability: capability.into(),
    }
}

fn merge_abi_color(color: &mut NativeFrameColorMetadata, cicp: VesperColorInfo) {
    fn label(code: u8, known: &[(u8, &str)]) -> Option<String> {
        if code == 2 {
            return None;
        }
        Some(match known.iter().find(|(value, _)| *value == code) {
            Some((_, name)) => (*name).to_owned(),
            None => format!("cicp-{code}"),
        })
    }

    color.primaries = label(
        cicp.primaries,
        &[
            (1, "bt709"),
            (4, "bt470m"),
            (5, "bt470bg"),
            (6, "smpte170m"),
            (7, "smpte240m"),
            (8, "film"),
            (9, "bt2020"),
            (10, "smpte428"),
            (11, "smpte431"),
            (12, "smpte432"),
            (22, "jedec-p22"),
        ],
    )
    .or_else(|| color.primaries.take());
    color.transfer = label(
        cicp.transfer,
        &[
            (1, "bt709"),
            (4, "gamma22"),
            (5, "gamma28"),
            (6, "smpte170m"),
            (7, "smpte240m"),
            (8, "linear"),
            (9, "log100"),
            (10, "log316"),
            (11, "iec61966-2-4"),
            (12, "bt1361e"),
            (13, "iec61966-2-1"),
            (14, "bt2020-10"),
            (15, "bt2020-12"),
            (16, "smpte2084"),
            (17, "smpte428"),
            (18, "arib-std-b67"),
        ],
    )
    .or_else(|| color.transfer.take());
    color.matrix = label(
        cicp.matrix,
        &[
            (0, "gbr"),
            (1, "bt709"),
            (4, "fcc"),
            (5, "bt470bg"),
            (6, "smpte170m"),
            (7, "smpte240m"),
            (8, "ycgco"),
            (9, "bt2020-ncl"),
            (10, "bt2020-cl"),
            (11, "smpte2085"),
            (12, "chroma-derived-nc"),
            (13, "chroma-derived-c"),
            (14, "ictcp"),
        ],
    )
    .or_else(|| color.matrix.take());
    if cicp.bit_depth != 0 {
        color.bit_depth = Some(cicp.bit_depth);
    }
    match cicp.range {
        0 => {}
        1 => color.range = Some("limited".to_owned()),
        2 => color.range = Some("full".to_owned()),
        value => color.range = Some(format!("unknown-{value}")),
    }
}

fn map_error(error: PluginError) -> DecoderError {
    match error {
        PluginError::Contract(message) => DecoderError::AbiViolation {
            message: message.to_owned(),
        },
        PluginError::Status(VESPER_ERR_UNSUPPORTED) => {
            unsupported("native video decoder operation")
        }
        PluginError::StatusWithDetail(VESPER_ERR_UNSUPPORTED, detail) => unsupported(&detail),
        error => DecoderError::internal(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decoded_cicp_updates_known_values_and_preserves_missing_and_unmapped_codes() {
        let mut color = NativeFrameColorMetadata {
            primaries: Some("bt709".to_owned()),
            transfer: Some("bt709".to_owned()),
            matrix: None,
            range: Some("limited".to_owned()),
            bit_depth: Some(8),
        };
        merge_abi_color(
            &mut color,
            VesperColorInfo {
                primaries: 9,
                transfer: 2,
                matrix: 81,
                range: 2,
                bit_depth: 10,
                ..Default::default()
            },
        );
        assert_eq!(color.primaries.as_deref(), Some("bt2020"));
        assert_eq!(color.transfer.as_deref(), Some("bt709"));
        assert_eq!(color.matrix.as_deref(), Some("cicp-81"));
        assert_eq!(color.range.as_deref(), Some("full"));
        assert_eq!(color.bit_depth, Some(10));
    }
}
