use std::{ffi::c_void, path::Path, ptr, ptr::NonNull};

use anyhow::{Context, Result, ensure};
use libloading::Library;

use crate::{abi::*, dsp::*};

type Destroy = unsafe extern "C" fn(*mut c_void);

pub struct DynamicGain {
    session: NonNull<c_void>,
    api: VesperProbeGainApi,
    destroy: Destroy,
    _library: Library,
}

impl DynamicGain {
    /// Loads the independently built fixture, whose code and pointers are trusted.
    pub unsafe fn open(path: &Path, gain: f32) -> Result<Self> {
        // SAFETY: the caller supplies the trusted probe fixture.
        let library = unsafe { Library::new(path) }.context("load C++ gain fixture")?;
        type Entry = unsafe extern "C" fn() -> *const VesperProbeGainApi;
        // SAFETY: the fixture exports this symbol with the private probe signature.
        let entry = unsafe { library.get::<Entry>(b"vesper_probe_gain_api\0") }?;
        // SAFETY: the library is kept alive and the checked fixture exposes the entry ABI.
        let api = unsafe { entry() };
        ensure!(
            !api.is_null() && api.is_aligned(),
            "invalid audio table address"
        );
        // SAFETY: the trusted entry returns a static table with a readable size prefix.
        let size = unsafe { api.cast::<u32>().read() } as usize;
        ensure!(
            size >= size_of::<VesperProbeGainApi>(),
            "truncated audio table"
        );
        // SAFETY: the checked size covers the immutable fixture table.
        let api = unsafe { api.read() };
        let open = api.open.context("missing audio open callback")?;
        let destroy = api.destroy.context("missing audio destroy callback")?;
        ensure!(
            api.process_block.is_some()
                && api.push.is_some()
                && api.pull.is_some()
                && api.flush.is_some()
                && api.set_playback_policy.is_some()
                && api.latency_frames.is_some(),
            "incomplete audio table"
        );

        let mut session = ptr::null_mut();
        // SAFETY: `session` is a writable out pointer. The fixture owns its allocation.
        let status = unsafe { open(gain, &mut session) };
        ensure!(status == VESPER_PROBE_OK, "audio open returned {status}");
        let session = NonNull::new(session).context("audio open returned a null session")?;
        Ok(Self {
            session,
            api,
            destroy,
            _library: library,
        })
    }

    pub fn set_policy(&mut self, rate: f32, epoch: u64) -> Result<(), DspError> {
        let set = self
            .api
            .set_playback_policy
            .ok_or(DspError::InvalidProgress)?;
        // SAFETY: this host uniquely owns the live session and its library.
        check_ok(unsafe { set(self.session.as_ptr(), rate, epoch) })
    }

    pub fn latency_frames(&mut self) -> Result<u32, DspError> {
        let latency = self.api.latency_frames.ok_or(DspError::InvalidProgress)?;
        // SAFETY: this host uniquely owns the live session and its library.
        Ok(unsafe { latency(self.session.as_ptr()) })
    }

    pub fn pull_segment(&mut self, output: &mut [StereoFrame]) -> Result<(Pull, u64), DspError> {
        let pull = self.api.pull.ok_or(DspError::InvalidProgress)?;
        let mut desc = audio_buffer(output.as_mut_ptr(), 0, output.len())?;
        let mut produced = 0;
        // SAFETY: the descriptor describes uniquely borrowed writable storage for this call.
        let status = unsafe { pull(self.session.as_ptr(), &mut desc, &mut produced) };
        if produced != desc.frame_count || produced as usize > output.len() {
            return Err(DspError::InvalidProgress);
        }
        match (status, produced) {
            (VESPER_PROBE_OK, count) if count > 0 => {
                Ok((Pull::Produced(count as usize), desc.rate_epoch))
            }
            (VESPER_PROBE_NEED_INPUT, 0) => Ok((Pull::NeedInput, 0)),
            (VESPER_PROBE_EOF, 0) => Ok((Pull::Eof, 0)),
            (VESPER_PROBE_OK | VESPER_PROBE_NEED_INPUT | VESPER_PROBE_EOF, _) => {
                Err(DspError::InvalidProgress)
            }
            (status, _) => Err(DspError::PluginStatus(status)),
        }
    }
}

impl Drop for DynamicGain {
    fn drop(&mut self) {
        // SAFETY: this owner calls destroy once before unloading the fixture.
        unsafe { (self.destroy)(self.session.as_ptr()) };
    }
}

impl AudioProcessor for DynamicGain {
    fn process_block(&mut self, block: Block<'_>) -> Result<(), DspError> {
        let process = self.api.process_block.ok_or(DspError::InvalidProgress)?;
        let (status, frames, expected) = match block {
            Block::InPlace(frames) => {
                let mut buffer = audio_buffer(frames.as_mut_ptr(), frames.len(), frames.len())?;
                let descriptor = &raw mut buffer;
                // SAFETY: the same descriptor and exclusively writable PCM are borrowed for
                // the call. The plugin ABI permits exact aliasing in this mode.
                let status = unsafe { process(self.session.as_ptr(), descriptor, descriptor) };
                (status, buffer.frame_count, frames.len())
            }
            Block::Separate(input, output) => {
                if input.len() != output.len() {
                    return Err(DspError::BlockLength);
                }
                let input_desc = audio_buffer(input.as_ptr().cast_mut(), input.len(), input.len())?;
                let mut output_desc = audio_buffer(output.as_mut_ptr(), 0, output.len())?;
                // SAFETY: input is read-only under the ABI and disjoint from the mutable output.
                // Both slices stay valid for the duration of the call.
                let status =
                    unsafe { process(self.session.as_ptr(), &input_desc, &mut output_desc) };
                (status, output_desc.frame_count, input.len())
            }
        };
        check_ok(status)?;
        if frames as usize != expected {
            return Err(DspError::InvalidProgress);
        }
        Ok(())
    }

    fn push(&mut self, input: Option<&[StereoFrame]>) -> Result<Push, DspError> {
        let push = self.api.push.ok_or(DspError::InvalidProgress)?;
        let mut consumed = 0;
        let desc = input
            .map(|frames| audio_buffer(frames.as_ptr().cast_mut(), frames.len(), frames.len()))
            .transpose()?;
        // SAFETY: input is a read-only borrow for this call, or NULL for EOS. The output
        // count is writable and initialized. The fixture copies whatever samples it retains.
        let status = unsafe {
            push(
                self.session.as_ptr(),
                desc.as_ref().map_or(ptr::null(), ptr::from_ref),
                &mut consumed,
            )
        };
        match (status, consumed, input) {
            (VESPER_PROBE_OK, 0, None) => Ok(Push::EndAccepted),
            (VESPER_PROBE_OK, count, Some(frames))
                if count > 0 && count as usize <= frames.len() =>
            {
                Ok(Push::Consumed(count as usize))
            }
            (VESPER_PROBE_TRY_AGAIN, 0, Some(_)) => Ok(Push::Backpressure),
            (VESPER_PROBE_OK | VESPER_PROBE_TRY_AGAIN, _, _) => Err(DspError::InvalidProgress),
            (status, _, _) => Err(DspError::PluginStatus(status)),
        }
    }

    fn pull(&mut self, output: &mut [StereoFrame]) -> Result<Pull, DspError> {
        self.pull_segment(output).map(|(result, _)| result)
    }

    fn flush(&mut self) -> Result<(), DspError> {
        let flush = self.api.flush.ok_or(DspError::InvalidProgress)?;
        // SAFETY: this is the unique session owner and the library is still loaded.
        check_ok(unsafe { flush(self.session.as_ptr()) })
    }
}

fn check_ok(status: u32) -> Result<(), DspError> {
    if status == VESPER_PROBE_OK {
        Ok(())
    } else {
        Err(DspError::PluginStatus(status))
    }
}

fn audio_buffer(
    data: *mut StereoFrame,
    count: usize,
    capacity: usize,
) -> Result<VesperProbeAudioBuffer, DspError> {
    let mut planes = [ptr::null_mut(); 8];
    planes[0] = data.cast();
    Ok(VesperProbeAudioBuffer {
        struct_size: size_of::<VesperProbeAudioBuffer>() as u32,
        format_id: VESPER_PROBE_F32_STEREO_FORMAT,
        pts_us: 0,
        rate_epoch: 0,
        flags: 0,
        frame_count: u32::try_from(count).map_err(|_| DspError::BlockLength)?,
        frame_capacity: u32::try_from(capacity).map_err(|_| DspError::BlockLength)?,
        planes,
    })
}
