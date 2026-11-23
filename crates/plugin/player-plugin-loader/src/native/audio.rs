use std::num::NonZeroU16;
use std::ptr::NonNull;
use std::time::Instant;

use super::*;

pub(super) struct Factory {
    root: Arc<Root>,
    table: VesperAudioProcessorVtable,
    capabilities_json: String,
    modes: AudioProcessorCapabilities,
    call_budget: Duration,
}

impl Factory {
    pub(super) fn new(
        root: Arc<Root>,
        table: VesperAudioProcessorVtable,
        call_budget: Duration,
    ) -> PluginResult<Self> {
        let modes = AudioProcessorCapabilities {
            fixed_block: table.process_block.is_some(),
            buffered: table.push.is_some() && table.pull.is_some(),
        };
        if table.open.is_none()
            || table.destroy.is_none()
            || table.flush.is_none()
            || table.latency_frames.is_none()
            || table.set_playback_policy.is_none()
            || table.push.is_some() != table.pull.is_some()
            || (!modes.fixed_block && !modes.buffered)
            || call_budget.is_zero()
        {
            return Err(PluginError::Contract(
                "incomplete audio processor table or budget",
            ));
        }
        let query = table
            .query_capabilities_json
            .ok_or(PluginError::Contract("missing audio capabilities query"))?;
        let mut out = VesperBytes::default();
        // SAFETY: root and the table are live; out has writable capacity.
        check_status(unsafe { query(root.0.owner, &mut out) })?;
        // SAFETY: result points to root-owned borrowed bytes.
        let capabilities_json = std::str::from_utf8(unsafe { read_bytes(out, MAX_JSON_BYTES) }?)
            .map_err(|_| PluginError::Contract("capabilities are not UTF-8"))?
            .to_owned();
        Ok(Self {
            root,
            table,
            capabilities_json,
            modes,
            call_budget,
        })
    }
}

impl AudioProcessorFactory for Factory {
    fn capabilities(&self) -> AudioProcessorCapabilities {
        self.modes
    }
    fn capabilities_json(&self) -> &str {
        &self.capabilities_json
    }

    fn open(&self, input: VesperAudioFormat, params: &[u8]) -> PluginResult<OpenAudioProcessor> {
        let in_channels = validate_f32_audio_format(&input)?;
        let open = self
            .table
            .open
            .ok_or(PluginError::Contract("missing audio open callback"))?;
        let destroy = self
            .table
            .destroy
            .ok_or(PluginError::Contract("missing audio destroy callback"))?;
        let mut output = VesperAudioFormat {
            struct_size: size_of::<VesperAudioFormat>() as u32,
            ..VesperAudioFormat::default()
        };
        let mut latency_frames = 0;
        let mut raw = ptr::null_mut();
        // SAFETY: callbacks stay mapped, arguments are live and outputs are writable.
        let status = unsafe {
            open(
                self.root.0.owner,
                &input,
                bytes(params),
                &mut output,
                &mut latency_frames,
                &mut raw,
            )
        };
        check_status(status)?;
        let raw =
            NonNull::new(raw).ok_or(PluginError::Contract("audio open returned no session"))?;
        let session = RawSession {
            raw,
            destroy,
            _root: Arc::clone(&self.root),
        };
        let out_channels = validate_f32_audio_format(&output)?;
        if self.modes.fixed_block
            && (input.channels != output.channels || input.sample_rate != output.sample_rate)
        {
            return Err(PluginError::Contract(
                "fixed-block DSP changed audio layout",
            ));
        }
        Ok(OpenAudioProcessor {
            output_format: output,
            processor: Box::new(Processor {
                session,
                input,
                output,
                in_channels,
                out_channels,
                latency_frames,
                table: self.table,
                modes: self.modes,
                call_budget: self.call_budget,
                failed: false,
                eos: false,
                eof: false,
                highest_input_epoch: 0,
                highest_output_epoch: 0,
            }),
        })
    }
}

struct RawSession {
    raw: NonNull<VesperSession>,
    destroy: unsafe extern "C" fn(*mut VesperSession),
    _root: Arc<Root>,
}

// SAFETY: ABI sessions may move between threads and are exclusively owned.
unsafe impl Send for RawSession {}

impl Drop for RawSession {
    fn drop(&mut self) {
        // SAFETY: the successful open transferred this session once. The root is live.
        unsafe { (self.destroy)(self.raw.as_ptr()) };
    }
}

struct Processor {
    session: RawSession,
    table: VesperAudioProcessorVtable,
    input: VesperAudioFormat,
    output: VesperAudioFormat,
    in_channels: NonZeroU16,
    out_channels: NonZeroU16,
    modes: AudioProcessorCapabilities,
    latency_frames: u32,
    call_budget: Duration,
    failed: bool,
    eos: bool,
    eof: bool,
    highest_input_epoch: u64,
    highest_output_epoch: u64,
}

fn abi_buffer(
    format_id: u64,
    ptr: *mut f32,
    frames: u32,
    capacity: u32,
    time: AudioTime,
) -> VesperAudioBuffer {
    let mut buffer = VesperAudioBuffer {
        struct_size: size_of::<VesperAudioBuffer>() as u32,
        format_id,
        pts_us: time.pts_us,
        rate_epoch: time.rate_epoch,
        frame_count: frames,
        frame_capacity: capacity,
        ..VesperAudioBuffer::default()
    };
    buffer.planes[0] = ptr.cast();
    buffer
}

fn validate_frames(frames: usize, max: u16) -> PluginResult<u32> {
    u16::try_from(frames)
        .ok()
        .filter(|frames| *frames <= max)
        .map(u32::from)
        .ok_or(PluginError::Contract(
            "audio block exceeds negotiated capacity",
        ))
}

impl Processor {
    fn call(
        &mut self,
        invoke: impl FnOnce(*mut VesperSession) -> VesperStatus,
    ) -> PluginResult<VesperStatus> {
        if self.failed {
            return Err(PluginError::CircuitOpen);
        }
        let start = Instant::now();
        let status = invoke(self.session.raw.as_ptr());
        if start.elapsed() > self.call_budget || status == VESPER_ERR_PANIC {
            self.failed = true;
            if status == VESPER_OK {
                return Err(PluginError::CircuitOpen);
            }
        }
        Ok(status)
    }

    fn violation<T>(&mut self, detail: &'static str) -> PluginResult<T> {
        self.failed = true;
        Err(PluginError::Contract(detail))
    }
}

impl AudioProcessor for Processor {
    fn capabilities(&self) -> AudioProcessorCapabilities {
        self.modes
    }

    fn set_playback_policy(&mut self, rate: f32, pitch: u32, epoch: u64) -> PluginResult<()> {
        let call = self
            .table
            .set_playback_policy
            .ok_or(PluginError::Status(VESPER_ERR_UNSUPPORTED))?;
        if !rate.is_finite() || rate <= 0.0 || epoch == 0 {
            return Err(PluginError::Contract("invalid playback policy"));
        }
        // SAFETY: call operates on the live, exclusive session.
        check_status(self.call(|session| unsafe { call(session, rate, pitch, epoch) })?)
    }

    fn set_param(&mut self, key: &[u8], value: &[u8]) -> PluginResult<()> {
        let call = self
            .table
            .set_param
            .ok_or(PluginError::Status(VESPER_ERR_UNSUPPORTED))?;
        // SAFETY: call operates on the live session and borrows both slices.
        check_status(self.call(|session| unsafe { call(session, bytes(key), bytes(value)) })?)
    }

    fn process_block(&mut self, mut block: AudioBlock<'_>) -> PluginResult<()> {
        let call = self
            .table
            .process_block
            .ok_or(PluginError::Status(VESPER_ERR_UNSUPPORTED))?;
        let (mut out, input, time, frames);
        let input_ptr = match block {
            AudioBlock::InPlace {
                ref mut audio,
                time: value,
            } => {
                time = value;
                if audio.channels() != self.in_channels
                    || self.input.format_id != self.output.format_id
                {
                    return Err(PluginError::Contract(
                        "in-place DSP requires matching format and channels",
                    ));
                }
                frames = validate_frames(
                    audio.frames(),
                    self.input
                        .max_block_frames
                        .min(self.output.max_block_frames),
                )?;
                out = abi_buffer(
                    self.output.format_id,
                    audio.samples().as_mut_ptr(),
                    frames,
                    frames,
                    time,
                );
                ptr::addr_of_mut!(out).cast_const()
            }
            AudioBlock::Split {
                input: view,
                ref mut output,
            } => {
                time = view.time;
                frames = validate_frames(view.frames(), self.input.max_block_frames)?;
                let capacity = validate_frames(output.frames(), self.output.max_block_frames)?;
                if view.channels() != self.in_channels
                    || output.channels() != self.out_channels
                    || frames > capacity
                {
                    return Err(PluginError::Contract("fixed-block DSP buffer mismatch"));
                }
                input = abi_buffer(
                    self.input.format_id,
                    view.samples().as_ptr().cast_mut(),
                    frames,
                    frames,
                    time,
                );
                out = abi_buffer(
                    self.output.format_id,
                    output.samples().as_mut_ptr(),
                    0,
                    capacity,
                    time,
                );
                ptr::addr_of!(input)
            }
        };
        if time.rate_epoch == 0 {
            return Err(PluginError::Contract("missing input audio epoch"));
        }
        let previous = out;
        // SAFETY: the session owns call access and the descriptor pointers refer to
        // either one writable in-place block or a disjoint read/write pair.
        check_status(self.call(|session| unsafe { call(session, input_ptr, &raw mut out) })?)?;
        if !same_storage(&previous, &out)
            || out.frame_count != frames
            || out.rate_epoch != time.rate_epoch
            || out.pts_us != time.pts_us
        {
            return self.violation("fixed-block DSP changed output storage, frame count or timing");
        }
        Ok(())
    }

    fn push(&mut self, input: Option<AudioBufferView<'_>>) -> PluginResult<AudioPush> {
        let call = self
            .table
            .push
            .ok_or(PluginError::Status(VESPER_ERR_UNSUPPORTED))?;
        if self.eos {
            return if input.is_none() {
                Ok(AudioPush::EosAccepted)
            } else {
                Err(PluginError::Contract("audio input after EOS"))
            };
        }
        let buffer = input
            .map(|view| {
                let frames = validate_frames(view.frames(), self.input.max_block_frames)?;
                if view.channels() != self.in_channels
                    || frames == 0
                    || view.time.rate_epoch == 0
                    || view.time.rate_epoch < self.highest_input_epoch
                {
                    return Err(PluginError::Contract("invalid buffered audio input"));
                }
                Ok(abi_buffer(
                    self.input.format_id,
                    view.samples().as_ptr().cast_mut(),
                    frames,
                    frames,
                    view.time,
                ))
            })
            .transpose()?;
        let ptr = buffer.as_ref().map_or(ptr::null(), ptr::from_ref);
        let mut consumed = 0;
        // SAFETY: input is either null for EOS or a borrowed, read-only block.
        let status = self.call(|session| unsafe { call(session, ptr, &mut consumed) })?;
        match (status, buffer, consumed) {
            (VESPER_OK, None, 0) => {
                self.eos = true;
                Ok(AudioPush::EosAccepted)
            }
            (VESPER_OK, Some(input), count) if count > 0 && count <= input.frame_count => {
                self.highest_input_epoch = input.rate_epoch;
                Ok(AudioPush::Consumed(count))
            }
            (VESPER_TRY_AGAIN, _, 0) => Ok(AudioPush::Backpressure),
            (VESPER_OK | VESPER_TRY_AGAIN, _, _) => {
                self.violation("invalid or zero audio consumption")
            }
            (code, _, _) => Err(PluginError::Status(code)),
        }
    }

    fn pull(&mut self, mut output: AudioBufferMut<'_>) -> PluginResult<AudioPull> {
        let call = self
            .table
            .pull
            .ok_or(PluginError::Status(VESPER_ERR_UNSUPPORTED))?;
        if self.eof {
            return Ok(AudioPull::Eof);
        }
        let capacity = validate_frames(output.frames(), self.output.max_block_frames)?;
        if output.channels() != self.out_channels || capacity == 0 {
            return Err(PluginError::Contract("invalid buffered audio output"));
        }
        let mut out = abi_buffer(
            self.output.format_id,
            output.samples().as_mut_ptr(),
            0,
            capacity,
            AudioTime {
                pts_us: i64::MIN,
                rate_epoch: 0,
            },
        );
        let before = out;
        let mut produced = 0;
        // SAFETY: out and produced are writable. The PCM storage remains owned by output.
        let code = self.call(|session| unsafe { call(session, &mut out, &mut produced) })?;
        if !same_storage(&before, &out) {
            return self.violation("audio plugin replaced output storage");
        }
        match (code, produced) {
            (VESPER_OK, count)
                if count > 0
                    && count <= capacity
                    && out.frame_count == count
                    && out.rate_epoch != 0
                    && out.rate_epoch >= self.highest_output_epoch
                    && out.rate_epoch <= self.highest_input_epoch =>
            {
                self.highest_output_epoch = out.rate_epoch;
                Ok(AudioPull::Produced {
                    frames: count,
                    time: AudioTime {
                        pts_us: out.pts_us,
                        rate_epoch: out.rate_epoch,
                    },
                })
            }
            (VESPER_NEED_MORE_INPUT, 0) if !self.eos => Ok(AudioPull::NeedInput),
            (VESPER_TRY_AGAIN, 0) => Ok(AudioPull::TryAgain),
            (VESPER_EOF, 0) if self.eos => {
                self.eof = true;
                Ok(AudioPull::Eof)
            }
            (VESPER_OK | VESPER_EOF | VESPER_TRY_AGAIN | VESPER_NEED_MORE_INPUT, _) => {
                self.violation("invalid buffered audio output or epoch")
            }
            (code, _) => Err(PluginError::Status(code)),
        }
    }

    fn latency_frames(&self) -> u32 {
        self.latency_frames
    }

    fn flush(&mut self) -> PluginResult<()> {
        let call = self
            .table
            .flush
            .ok_or(PluginError::Status(VESPER_ERR_UNSUPPORTED))?;
        // SAFETY: callback and exclusive session remain live.
        check_status(self.call(|session| unsafe { call(session) })?)?;
        self.eos = false;
        self.eof = false;
        self.highest_input_epoch = 0;
        self.highest_output_epoch = 0;
        Ok(())
    }
}

fn same_storage(before: &VesperAudioBuffer, after: &VesperAudioBuffer) -> bool {
    before.planes == after.planes
        && before.frame_capacity == after.frame_capacity
        && before.format_id == after.format_id
        && after.struct_size == before.struct_size
}
