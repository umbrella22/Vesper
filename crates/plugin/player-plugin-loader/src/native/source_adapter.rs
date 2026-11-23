use super::*;
use player_plugin::{
    SourceNormalizerError, SourceNormalizerOperationStatus, SourceNormalizerPacket,
    SourceNormalizerPacketCapabilities, SourceNormalizerPacketLease,
    SourceNormalizerPacketPluginFactory, SourceNormalizerPacketSeek, SourceNormalizerPacketSession,
    SourceNormalizerPacketSessionConfig, SourceNormalizerPacketStreamInfo,
    SourceNormalizerReadPacketMetadata, SourceNormalizerResourceCapabilities,
    SourceNormalizerResourcePluginFactory, SourceNormalizerResourceSession,
    SourceNormalizerResourceSessionConfig, SourceNormalizerResourceSessionInfo,
    SourceNormalizerResourceSessionStatus, SourceNormalizerResourceSessionWaitStatus,
};

struct SourceFactory<C> {
    plugin: NativePlugin,
    capabilities: C,
}

impl NativePlugin {
    pub(crate) fn packet_factory(
        &self,
    ) -> PluginResult<Arc<dyn SourceNormalizerPacketPluginFactory>> {
        Ok(Arc::new(SourceFactory {
            plugin: self.clone(),
            capabilities: self.packet_capabilities()?,
        }))
    }

    pub(crate) fn resource_factory(
        &self,
    ) -> PluginResult<Arc<dyn SourceNormalizerResourcePluginFactory>> {
        Ok(Arc::new(SourceFactory {
            plugin: self.clone(),
            capabilities: self.resource_capabilities()?,
        }))
    }
}

impl SourceNormalizerPacketPluginFactory for SourceFactory<SourceNormalizerPacketCapabilities> {
    fn name(&self) -> &str {
        self.plugin.name()
    }

    fn packet_capabilities(&self) -> SourceNormalizerPacketCapabilities {
        self.capabilities.clone()
    }

    fn open_packet_session(
        &self,
        config: &SourceNormalizerPacketSessionConfig,
    ) -> Result<Box<dyn SourceNormalizerPacketSession>, SourceNormalizerError> {
        self.plugin
            .open_packet_stream(config)
            .map(NativePacketStream::into_rust_session)
            .map_err(operation_error)
    }
}

impl SourceNormalizerResourcePluginFactory for SourceFactory<SourceNormalizerResourceCapabilities> {
    fn name(&self) -> &str {
        self.plugin.name()
    }

    fn resource_capabilities(&self) -> SourceNormalizerResourceCapabilities {
        self.capabilities.clone()
    }

    fn open_resource_session(
        &self,
        config: &SourceNormalizerResourceSessionConfig,
    ) -> Result<Box<dyn SourceNormalizerResourceSession>, SourceNormalizerError> {
        self.plugin
            .open_source_resource(config)
            .and_then(NativeSourceResource::into_rust_session)
            .map_err(operation_error)
    }
}

impl NativePacketStream {
    /// Adapts packet leases to callers using the existing Rust source interface.
    /// The adapter retains the actual ABI lease without copying its bytes.
    pub fn into_rust_session(self) -> Box<dyn SourceNormalizerPacketSession> {
        Box::new(PacketSession {
            info: self.stream_info().clone(),
            stream: Some(self),
            outstanding: None,
            last_handle: 0,
        })
    }
}

impl NativeSourceResource {
    /// Retains the ABI resource owner while an existing host consumes its output.
    pub fn into_rust_session(mut self) -> PluginResult<Box<dyn SourceNormalizerResourceSession>> {
        if !self.supports_resource_control() {
            return Err(PluginError::Contract(
                "normalized resource lacks wait or cancellation",
            ));
        }
        let status = self.poll()?;
        let info = status
            .info
            .ok_or(PluginError::Contract("missing normalized resource info"))?;
        Ok(Box::new(ResourceSession {
            native: Some(self),
            info,
        }))
    }
}

fn operation_error(error: PluginError) -> SourceNormalizerError {
    match error {
        PluginError::Contract(message) => SourceNormalizerError::abi_violation(message),
        error => SourceNormalizerError::internal(error.to_string()),
    }
}

fn completion(completed: bool) -> SourceNormalizerOperationStatus {
    SourceNormalizerOperationStatus {
        completed,
        message: None,
    }
}

struct PacketSession {
    info: SourceNormalizerPacketStreamInfo,
    stream: Option<NativePacketStream>,
    outstanding: Option<NativeNormalizedPacket>,
    last_handle: usize,
}

impl SourceNormalizerPacketSession for PacketSession {
    fn stream_info(&self) -> SourceNormalizerPacketStreamInfo {
        self.info.clone()
    }

    fn read_packet(&mut self) -> Result<SourceNormalizerPacketLease<'_>, SourceNormalizerError> {
        if self.outstanding.is_some() {
            return Ok(SourceNormalizerPacketLease {
                metadata: SourceNormalizerReadPacketMetadata::need_more_data(None),
                data: &[],
                handle: 0,
            });
        }
        let stream = self
            .stream
            .as_mut()
            .ok_or(SourceNormalizerError::NotConfigured)?;
        let poll = stream.read().map_err(operation_error)?;
        let packet = match poll {
            NativePacketStreamPoll::Packet(packet) => packet,
            NativePacketStreamPoll::Pending | NativePacketStreamPoll::EndOfStream => {
                return Ok(SourceNormalizerPacketLease {
                    metadata: match poll {
                        NativePacketStreamPoll::Pending => {
                            SourceNormalizerReadPacketMetadata::need_more_data(None)
                        }
                        _ => SourceNormalizerReadPacketMetadata::end_of_stream(),
                    },
                    data: &[],
                    handle: 0,
                });
            }
        };
        self.last_handle = self
            .last_handle
            .checked_add(1)
            .ok_or_else(|| SourceNormalizerError::internal("packet handle space exhausted"))?;
        let raw = packet.descriptor();
        let track = self
            .info
            .tracks
            .iter()
            .find(|track| track.stream_index == raw.stream_index);
        let timestamp = |raw| (raw != i64::MIN).then_some(raw);
        let metadata = SourceNormalizerReadPacketMetadata::packet(SourceNormalizerPacket {
            pts_us: timestamp(raw.pts_us),
            dts_us: timestamp(raw.dts_us),
            duration_us: timestamp(raw.duration_us),
            stream_index: raw.stream_index,
            media_kind: track.map_or_else(Default::default, |value| value.media_kind),
            key_frame: raw.flags & VESPER_PACKET_KEYFRAME != 0,
            discontinuity: raw.flags & VESPER_PACKET_DISCONTINUITY != 0,
            end_of_stream: raw.flags & VESPER_PACKET_EOS != 0,
            ..Default::default()
        });
        let packet: &NativeNormalizedPacket = self.outstanding.insert(packet);
        Ok(SourceNormalizerPacketLease {
            metadata,
            data: packet.as_ref(),
            handle: self.last_handle,
        })
    }

    fn release_packet(&mut self, handle: usize) -> Result<(), SourceNormalizerError> {
        if handle != self.last_handle || self.outstanding.is_none() {
            return Err(SourceNormalizerError::abi_violation("stale packet handle"));
        }
        self.outstanding.take();
        Ok(())
    }

    fn seek(
        &mut self,
        seek: &SourceNormalizerPacketSeek,
    ) -> Result<SourceNormalizerOperationStatus, SourceNormalizerError> {
        self.stream
            .as_mut()
            .ok_or(SourceNormalizerError::NotConfigured)?
            .seek(Duration::from_millis(seek.position_millis))
            .map_err(operation_error)?;
        Ok(completion(true))
    }

    fn flush(&mut self) -> Result<SourceNormalizerOperationStatus, SourceNormalizerError> {
        self.stream
            .as_mut()
            .ok_or(SourceNormalizerError::NotConfigured)?
            .flush()
            .map_err(operation_error)?;
        Ok(completion(true))
    }

    fn close(&mut self) -> Result<(), SourceNormalizerError> {
        self.outstanding.take();
        self.stream.take();
        Ok(())
    }
}

struct ResourceSession {
    native: Option<NativeSourceResource>,
    info: SourceNormalizerResourceSessionInfo,
}

impl ResourceSession {
    fn native(&mut self) -> Result<&mut NativeSourceResource, SourceNormalizerError> {
        self.native
            .as_mut()
            .ok_or(SourceNormalizerError::NotConfigured)
    }
}

impl SourceNormalizerResourceSession for ResourceSession {
    fn session_info(&self) -> SourceNormalizerResourceSessionInfo {
        self.info.clone()
    }

    fn poll(&mut self) -> Result<SourceNormalizerResourceSessionStatus, SourceNormalizerError> {
        let status = self.native()?.poll().map_err(operation_error)?;
        if let Some(info) = &status.info {
            self.info = info.clone();
        }
        Ok(status)
    }

    fn wait_for_update(
        &mut self,
        timeout: Duration,
    ) -> Result<SourceNormalizerResourceSessionWaitStatus, SourceNormalizerError> {
        let updated = self
            .native()?
            .wait_for_update(timeout)
            .map_err(operation_error)?;
        Ok(SourceNormalizerResourceSessionWaitStatus { updated })
    }

    fn cancel(&mut self) -> Result<SourceNormalizerOperationStatus, SourceNormalizerError> {
        let completed = self.native()?.cancel().map_err(operation_error)?;
        Ok(completion(completed))
    }

    fn close(&mut self) -> Result<(), SourceNormalizerError> {
        self.native.take();
        Ok(())
    }
}
