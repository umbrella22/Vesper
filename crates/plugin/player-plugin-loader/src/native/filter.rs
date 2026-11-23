use super::*;
use std::ptr::NonNull;
use std::time::Instant;

pub struct NativeVideoFilter {
    root: Arc<Root>,
    table: VesperVideoFilterVtable,
    session: NonNull<VesperSession>,
    input_format: u64,
    output_format: VesperVideoFormat,
    budget: Duration,
    poisoned: bool,
}

// SAFETY: the ABI permits transferring a session with exclusive ownership.
unsafe impl Send for NativeVideoFilter {}

pub enum VideoFilterOutput {
    Frame(VesperVideoFrame),
    Passthrough,
    Pending,
    EndOfStream,
    FormatChanged(VesperVideoFormat),
}

/// Records an output lease independently of the operation status. An error can
/// still follow GPU encoding, so the caller retires the lease with the command.
#[must_use]
pub struct RecordedVideoFilterOutput {
    pub output: PluginResult<VideoFilterOutput>,
    pub lease: Option<NativeVideoFrame>,
}

impl NativePlugin {
    /// Opens a filter for a negotiated input format and one host GPU device.
    ///
    /// # Safety
    /// GPU handles must be live, thread-valid for the call and native to the
    /// backend discriminator. The caller owns the device through session drop.
    pub unsafe fn open_video_filter(
        &self,
        gpu: &VesperGpuContext,
        input: VesperVideoFormat,
        params: &[u8],
        budget: Duration,
    ) -> PluginResult<NativeVideoFilter> {
        validate_gpu(gpu)?;
        validate_video_format(&input)?;
        if budget.is_zero() || params.len() > MAX_JSON_BYTES {
            return Err(PluginError::Contract("invalid video filter open options"));
        }
        let raw = self
            .query(&VESPER_VIDEO_FILTER_ID)?
            .cast::<VesperVideoFilterVtable>();
        // SAFETY: only the nullable diagnostic suffix can be omitted.
        let table =
            unsafe { read_vtable(raw, offset_of!(VesperVideoFilterVtable, get_last_error)) }?;
        let (Some(open), Some(destroy), Some(_), Some(_), Some(_)) = (
            table.open,
            table.destroy,
            table.process,
            table.get_format,
            table.flush,
        ) else {
            return Err(PluginError::Contract("incomplete video filter table"));
        };
        let mut output = empty_format();
        let mut session = ptr::null_mut();
        let start = Instant::now();
        // SAFETY: the caller guarantees the device; format and parameters are borrowed.
        let status = unsafe {
            open(
                self.root.0.owner,
                gpu,
                &input,
                bytes(params),
                &mut output,
                &mut session,
            )
        };
        let result = check_status(status).and_then(|()| {
            if start.elapsed() > budget {
                return Err(PluginError::Status(VESPER_ERR_ABORTED));
            }
            validate_video_format(&output)
        });
        if let Err(error) = result {
            if !session.is_null() {
                // SAFETY: a non-null session output must be destroyable even on error.
                unsafe { destroy(session) };
            }
            return Err(error);
        }
        Ok(NativeVideoFilter {
            root: Arc::clone(&self.root),
            table,
            session: NonNull::new(session).ok_or(PluginError::Contract("null filter session"))?,
            input_format: input.format_id,
            output_format: output,
            budget,
            poisoned: false,
        })
    }
}

impl NativeVideoFilter {
    pub fn output_format(&self) -> VesperVideoFormat {
        self.output_format
    }

    pub fn set_param(&mut self, key: &str, value: &[u8]) -> PluginResult<()> {
        self.check_live()?;
        let set = self
            .table
            .set_param
            .ok_or(PluginError::Status(VESPER_ERR_UNSUPPORTED))?;
        let start = Instant::now();
        // SAFETY: parameters are borrowed for this exclusive session operation.
        let status = unsafe { set(self.session.as_ptr(), bytes(key.as_bytes()), bytes(value)) };
        check_status(self.status(start, status)?)
    }

    pub fn flush(&mut self) -> PluginResult<()> {
        self.check_live()?;
        let flush = self
            .table
            .flush
            .ok_or(PluginError::Contract("missing filter flush"))?;
        let start = Instant::now();
        // SAFETY: the session is live and has exclusive access.
        let status = unsafe { flush(self.session.as_ptr()) };
        check_status(self.status(start, status)?)
    }

    /// Records into a host command buffer. A null input drains queued outputs.
    /// An optional host target is borrowed and has no plugin release callback.
    ///
    /// # Safety
    /// GPU objects, inputs, and target handles must belong to the owning GPU
    /// backend. The caller keeps all referenced handles and the returned lease
    /// alive until submitted work completes, even on error. A frame returned
    /// here without a lease is backed by the input or the target supplied by the
    /// caller. The caller abandons an unsubmitted command before releasing it.
    pub unsafe fn encode(
        &mut self,
        gpu: &VesperGpuContext,
        input: Option<&VesperVideoFrame>,
        target: Option<&VesperVideoFrame>,
    ) -> RecordedVideoFilterOutput {
        let mut lease = None;
        let output = (|| {
            self.check_live()?;
            validate_gpu(gpu)?;
            if let Some(input) = input {
                validate_frame(input, self.input_format)?;
            }
            if let Some(target) = target {
                validate_frame(target, self.output_format.format_id)?;
                if target.lease_id != 0 || target.release_fn.is_some() {
                    return Err(PluginError::Contract(
                        "filter target must be a host-owned slot",
                    ));
                }
            }
            let process = self
                .table
                .process
                .ok_or(PluginError::Contract("missing filter process"))?;
            let mut frame = target.copied().unwrap_or_else(|| VesperVideoFrame {
                struct_size: size_of::<VesperVideoFrame>() as u32,
                ..Default::default()
            });
            let start = Instant::now();
            // SAFETY: caller guarantees live GPU objects and lifetime through retirement.
            let status = unsafe {
                process(
                    self.session.as_ptr(),
                    gpu,
                    input.map_or(ptr::null(), ptr::from_ref),
                    &mut frame,
                )
            };
            let aliases_input = input.is_some_and(|input| {
                input.lease_id != 0
                    && input.lease_id == frame.lease_id
                    && input.release_ctx == frame.release_ctx
            });
            let has_lease = frame.lease_id != 0 || frame.release_fn.is_some();
            if frame.lease_id != 0 && frame.release_fn.is_some() && !aliases_input {
                lease = Some(NativeVideoFrame::adopt(frame, Arc::clone(&self.root)));
            }
            let status = self.status(start, status)?;
            if has_lease && (status != VESPER_OK || lease.is_none()) {
                self.poisoned = true;
                return Err(PluginError::Contract("invalid video filter lease transfer"));
            }
            match status {
                VESPER_OK => {
                    if validate_frame(&frame, self.output_format.format_id).is_err()
                        || (lease.is_none()
                            && !target.is_some_and(|target| {
                                frame.handle_kind == target.handle_kind
                                    && frame.handles == target.handles
                                    && frame.plane_count == target.plane_count
                            }))
                    {
                        self.poisoned = true;
                        return Err(PluginError::Contract("invalid video filter output"));
                    }
                    // The separate RAII lease owns the sole release right.
                    frame.release_fn = None;
                    frame.release_ctx = ptr::null_mut();
                    frame.lease_id = 0;
                    Ok(VideoFilterOutput::Frame(frame))
                }
                VESPER_PASSTHROUGH if input.is_some() => Ok(VideoFilterOutput::Passthrough),
                VESPER_NEED_MORE_INPUT | VESPER_TRY_AGAIN => Ok(VideoFilterOutput::Pending),
                VESPER_EOF => Ok(VideoFilterOutput::EndOfStream),
                VESPER_FORMAT_CHANGED => self
                    .read_format_change()
                    .map(VideoFilterOutput::FormatChanged),
                code => Err(PluginError::Status(code)),
            }
        })();
        RecordedVideoFilterOutput { output, lease }
    }

    fn read_format_change(&mut self) -> PluginResult<VesperVideoFormat> {
        let get = self
            .table
            .get_format
            .ok_or(PluginError::Contract("missing filter format"))?;
        let mut output = empty_format();
        let start = Instant::now();
        // SAFETY: this is the live session's writable format output.
        let status = unsafe { get(self.session.as_ptr(), &mut output) };
        check_status(self.status(start, status)?)?;
        if validate_video_format(&output).is_err()
            || output.format_id <= self.output_format.format_id
        {
            self.poisoned = true;
            return Err(PluginError::Contract(
                "invalid or stale video filter format",
            ));
        }
        self.output_format = output;
        Ok(output)
    }

    fn check_live(&self) -> PluginResult<()> {
        if self.poisoned {
            return Err(PluginError::Status(VESPER_ERR_ABORTED));
        }
        Ok(())
    }

    fn status(&mut self, start: Instant, status: VesperStatus) -> PluginResult<VesperStatus> {
        self.poisoned = matches!(status, VESPER_ERR_PANIC | VESPER_ERR_GPU_LOST)
            || start.elapsed() > self.budget;
        if self.poisoned && !matches!(status, VESPER_ERR_PANIC | VESPER_ERR_GPU_LOST) {
            return Err(PluginError::Status(VESPER_ERR_ABORTED));
        }
        Ok(status)
    }
}

impl Drop for NativeVideoFilter {
    fn drop(&mut self) {
        if let Some(destroy) = self.table.destroy {
            // SAFETY: this is the unique owner of the opened filter session.
            unsafe { destroy(self.session.as_ptr()) };
        }
    }
}

fn empty_format() -> VesperVideoFormat {
    VesperVideoFormat {
        struct_size: size_of::<VesperVideoFormat>() as u32,
        ..Default::default()
    }
}

fn validate_video_format(format: &VesperVideoFormat) -> PluginResult<()> {
    if format.struct_size < size_of::<VesperVideoFormat>() as u32
        || format.format_id == 0
        || format.pixel_format == 0
        || format.width == 0
        || format.height == 0
    {
        return Err(PluginError::Contract("invalid video filter format"));
    }
    Ok(())
}

fn validate_frame(frame: &VesperVideoFrame, format_id: u64) -> PluginResult<()> {
    if frame.struct_size < size_of::<VesperVideoFrame>() as u32
        || frame.format_id != format_id
        || frame.handle_kind == 0
        || frame.plane_count == 0
        || frame.plane_count as usize > frame.handles.len()
        || frame.handles[0] == 0
    {
        return Err(PluginError::Contract(
            "invalid video frame for this filter format",
        ));
    }
    Ok(())
}

fn validate_gpu(gpu: &VesperGpuContext) -> PluginResult<()> {
    if gpu.struct_size < size_of::<VesperGpuContext>() as u32 {
        return Err(PluginError::Contract("incomplete GPU context"));
    }
    if !matches!(
        gpu.backend,
        VESPER_GPU_METAL | VESPER_GPU_VULKAN | VESPER_GPU_D3D11
    ) {
        return Err(PluginError::Status(VESPER_ERR_UNSUPPORTED));
    }
    Ok(())
}
