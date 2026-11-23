use super::*;
use std::ptr::NonNull;
use std::time::Instant;

pub struct NativeVideoDecoder {
    root: Arc<Root>,
    table: VesperVideoDecoderVtable,
    session: NonNull<VesperSession>,
    format: Option<VesperVideoFormat>,
    call_budget: Duration,
    poisoned: bool,
}

// SAFETY: the ABI permits transferring a session with exclusive ownership.
unsafe impl Send for NativeVideoDecoder {}

pub enum VideoDecoderOutput {
    Frame(NativeVideoFrame),
    FormatChanged(VesperVideoFormat),
    NeedMoreInput,
    TryAgain,
    EndOfStream,
}

/// A decoder lease can move to a GPU completion thread independently of its session.
pub struct NativeVideoFrame {
    raw: VesperVideoFrame,
    _root: Arc<Root>,
}

// SAFETY: the release callback contract allows exactly one release on any thread.
unsafe impl Send for NativeVideoFrame {}

impl NativeVideoFrame {
    pub(super) fn adopt(raw: VesperVideoFrame, root: Arc<Root>) -> Self {
        Self { raw, _root: root }
    }

    pub fn descriptor(&self) -> &VesperVideoFrame {
        &self.raw
    }

    pub fn complete(mut self, rendered: bool) {
        let disposition = if rendered {
            VESPER_RELEASE_RENDERED
        } else {
            VESPER_RELEASE_DROPPED
        };
        self.release(disposition);
    }

    fn release(&mut self, disposition: u32) {
        if let Some(release) = self.raw.release_fn.take()
            && self.raw.lease_id != 0
        {
            // SAFETY: the plugin transferred this lease with a live context and
            // one completion right. The root stays live through the call.
            unsafe { release(self.raw.release_ctx, self.raw.lease_id, disposition) };
        }
    }
}

impl Drop for NativeVideoFrame {
    fn drop(&mut self) {
        self.release(VESPER_RELEASE_DROPPED);
    }
}

impl NativeVideoDecoder {
    pub(super) fn open(
        root: Arc<Root>,
        table: VesperVideoDecoderVtable,
        gpu: Option<&VesperGpuContext>,
        codec_fourcc: u32,
        extradata: &[u8],
        params: &[u8],
        call_budget: Duration,
    ) -> PluginResult<Self> {
        let (Some(open), Some(destroy), Some(_), Some(_), Some(_), Some(_)) = (
            table.open,
            table.destroy,
            table.get_format,
            table.send_packet,
            table.receive_frame,
            table.flush,
        ) else {
            return Err(PluginError::Contract("incomplete video decoder table"));
        };
        if call_budget.is_zero() {
            return Err(PluginError::Contract("zero video decoder call budget"));
        }
        let mut session = ptr::null_mut();
        let start = Instant::now();
        // SAFETY: the caller keeps the optional GPU objects live for open. The
        // slices are readable for the call and the output slot is writable.
        let status = unsafe {
            open(
                root.0.owner,
                gpu.map_or(ptr::null(), ptr::from_ref),
                codec_fourcc,
                bytes(extradata),
                bytes(params),
                &mut session,
            )
        };
        if status != VESPER_OK || start.elapsed() > call_budget {
            let error = if status == VESPER_OK {
                PluginError::Status(VESPER_ERR_ABORTED)
            } else {
                decoder_error(&table, session, status)
            };
            if let Some(session) = NonNull::new(session) {
                // SAFETY: a non-null open output must describe a destroyable session.
                unsafe { destroy(session.as_ptr()) };
            }
            return Err(error);
        }
        Ok(Self {
            table,
            root,
            session: NonNull::new(session).ok_or(PluginError::Contract("null video session"))?,
            format: None,
            call_budget,
            poisoned: false,
        })
    }

    /// Returns false when the decoder needs output drained before retrying this packet.
    pub fn send_packet(&mut self, mut packet: VesperPacket, data: &[u8]) -> PluginResult<bool> {
        self.check_live()?;
        packet.struct_size = size_of::<VesperPacket>() as u32;
        packet.data = data.as_ptr();
        packet.len = data.len() as u64;
        let send = self
            .table
            .send_packet
            .ok_or(PluginError::Contract("missing send_packet"))?;
        let start = Instant::now();
        // SAFETY: the session is exclusively borrowed and packet bytes stay live through the call.
        let status = unsafe { send(self.session.as_ptr(), &packet) };
        match self.status(start, status)? {
            VESPER_OK => Ok(true),
            VESPER_TRY_AGAIN => Ok(false),
            value => Err(PluginError::Status(value)),
        }
    }

    pub fn receive(&mut self) -> PluginResult<VideoDecoderOutput> {
        self.check_live()?;
        let receive = self
            .table
            .receive_frame
            .ok_or(PluginError::Contract("missing receive_frame"))?;
        let mut raw = VesperVideoFrame {
            struct_size: size_of::<VesperVideoFrame>() as u32,
            ..Default::default()
        };
        let start = Instant::now();
        // SAFETY: the session is exclusively borrowed and raw is a sized writable output.
        let status = unsafe { receive(self.session.as_ptr(), &mut raw) };
        // Adopt the release before checking the time budget or descriptor contents.
        let lease = NativeVideoFrame {
            raw,
            _root: Arc::clone(&self.root),
        };
        let status = self.status(start, status)?;
        if status != VESPER_OK && (raw.lease_id != 0 || raw.release_fn.is_some()) {
            return self.violation("a non-frame status returned a video lease");
        }
        match status {
            VESPER_OK => {
                if raw.struct_size < size_of::<VesperVideoFrame>() as u32
                    || raw.lease_id == 0
                    || raw.release_fn.is_none()
                    || raw.plane_count == 0
                    || raw.plane_count > raw.handles.len() as u32
                    || raw.handle_kind == 0
                    || raw.handles[0] == 0
                    || self
                        .format
                        .is_none_or(|format| format.format_id != raw.format_id)
                {
                    return self.violation("invalid decoder frame or unnegotiated format");
                }
                Ok(VideoDecoderOutput::Frame(lease))
            }
            VESPER_FORMAT_CHANGED => self
                .read_new_format()
                .map(VideoDecoderOutput::FormatChanged),
            VESPER_NEED_MORE_INPUT => Ok(VideoDecoderOutput::NeedMoreInput),
            VESPER_TRY_AGAIN => Ok(VideoDecoderOutput::TryAgain),
            VESPER_EOF => Ok(VideoDecoderOutput::EndOfStream),
            value => Err(PluginError::Status(value)),
        }
    }

    fn read_new_format(&mut self) -> PluginResult<VesperVideoFormat> {
        let get = self
            .table
            .get_format
            .ok_or(PluginError::Contract("missing get_format"))?;
        let mut format = VesperVideoFormat {
            struct_size: size_of::<VesperVideoFormat>() as u32,
            ..Default::default()
        };
        let start = Instant::now();
        // SAFETY: the session and sized format output are live for this call.
        let status = unsafe { get(self.session.as_ptr(), &mut format) };
        check_status(self.status(start, status)?)?;
        if format.struct_size < size_of::<VesperVideoFormat>() as u32
            || format.format_id == 0
            || format.pixel_format == 0
            || format.width == 0
            || format.height == 0
            || self
                .format
                .is_some_and(|old| format.format_id <= old.format_id)
        {
            return self.violation("invalid or stale video format");
        }
        self.format = Some(format);
        Ok(format)
    }

    pub fn flush(&mut self) -> PluginResult<()> {
        self.check_live()?;
        let flush = self
            .table
            .flush
            .ok_or(PluginError::Contract("missing decoder flush"))?;
        let start = Instant::now();
        // SAFETY: the session is exclusively borrowed for this call.
        let status = unsafe { flush(self.session.as_ptr()) };
        let status = self.status(start, status)?;
        if status != VESPER_OK {
            return Err(decoder_error(&self.table, self.session.as_ptr(), status));
        }
        Ok(())
    }

    fn check_live(&self) -> PluginResult<()> {
        if self.poisoned {
            Err(PluginError::Status(VESPER_ERR_ABORTED))
        } else {
            Ok(())
        }
    }

    fn status(&mut self, start: Instant, status: VesperStatus) -> PluginResult<VesperStatus> {
        if status == VESPER_ERR_PANIC || start.elapsed() > self.call_budget {
            self.poisoned = true;
            return Err(PluginError::Status(if status == VESPER_ERR_PANIC {
                status
            } else {
                VESPER_ERR_ABORTED
            }));
        }
        Ok(status)
    }

    fn violation<T>(&mut self, message: &'static str) -> PluginResult<T> {
        self.poisoned = true;
        Err(PluginError::Contract(message))
    }
}

fn decoder_error(
    table: &VesperVideoDecoderVtable,
    session: *mut VesperSession,
    status: VesperStatus,
) -> PluginError {
    const MAX_DETAIL_BYTES: usize = 4096;
    if let Some(get_last_error) = table.get_last_error {
        let mut len = 0;
        // SAFETY: the decoder call just returned on this thread, and session
        // is its live output (or null after an unsuccessful open).
        let ptr = unsafe { get_last_error(session, &mut len) };
        if !ptr.is_null() && (1..=MAX_DETAIL_BYTES).contains(&(len as usize)) {
            // SAFETY: the ABI borrows this bounded span until the next call.
            let detail = unsafe { std::slice::from_raw_parts(ptr.cast::<u8>(), len as usize) };
            if let Ok(detail) = std::str::from_utf8(detail) {
                return PluginError::StatusWithDetail(status, detail.to_owned());
            }
        }
    }
    PluginError::Status(status)
}

impl Drop for NativeVideoDecoder {
    fn drop(&mut self) {
        if let Some(destroy) = self.table.destroy {
            // SAFETY: this object uniquely owns the successfully opened session.
            unsafe { destroy(self.session.as_ptr()) };
        }
    }
}
