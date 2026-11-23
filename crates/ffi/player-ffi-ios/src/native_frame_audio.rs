use std::ffi::c_char;
use std::num::NonZeroU16;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use player_platform_mobile::MAX_MOBILE_NATIVE_PLUGIN_ARTIFACTS_JSON_BYTES;
use player_plugin::author::{
    AudioBlock, AudioBufferMut, AudioProcessor, AudioProcessorFactory, AudioTime, PluginError,
    PluginResult, VesperAudioFormat,
};
use player_plugin_abi::{VESPER_PITCH_PRESERVE, VESPER_SAMPLE_F32_INTERLEAVED};

use crate::handles::HandleRegistry;
use crate::parse_mobile_native_plugin_artifacts_json;

// One call from Swift processes at most one block of its interleaving scratch.
const MAX_BLOCK_FRAMES: u16 = 1024;
const MAX_CHANNELS: u16 = 8;
const MAX_SESSIONS: usize = 32;
const NATIVE_CALL_BUDGET: Duration = Duration::from_millis(100);

type Slot = Arc<Mutex<Option<FixedBlockSession>>>;
static SESSIONS: LazyLock<Mutex<HandleRegistry<Slot>>> =
    LazyLock::new(|| Mutex::new(HandleRegistry::default()));

struct FixedBlockSession {
    processor: Box<dyn AudioProcessor>,
    channels: NonZeroU16,
    sample_rate: u32,
    block_frames: usize,
    epoch: u64,
}

impl FixedBlockSession {
    fn open(
        factory: &dyn AudioProcessorFactory,
        sample_rate: u32,
        channels: NonZeroU16,
        epoch: u64,
    ) -> PluginResult<Self> {
        if sample_rate == 0 || channels.get() > MAX_CHANNELS || epoch == 0 {
            return Err(PluginError::Contract(
                "invalid native-frame audio format or epoch",
            ));
        }
        if !factory.capabilities().fixed_block {
            return Err(PluginError::Contract(
                "native-frame iOS audio requires fixed blocks",
            ));
        }
        let input = VesperAudioFormat {
            struct_size: size_of::<VesperAudioFormat>() as u32,
            format_id: 1,
            sample_format: VESPER_SAMPLE_F32_INTERLEAVED,
            sample_rate,
            channels: channels.get(),
            channel_layout: 0,
            max_block_frames: MAX_BLOCK_FRAMES,
        };
        let opened = factory.open(input, &[])?;
        let output = opened.output_format;
        let mut processor = opened.processor;
        if !processor.capabilities().fixed_block
            || output.format_id != input.format_id
            || output.sample_format != input.sample_format
            || output.sample_rate != sample_rate
            || output.channels != channels.get()
            || output.channel_layout != input.channel_layout
            || output.max_block_frames == 0
            || processor.latency_frames() != 0
        {
            return Err(PluginError::Contract(
                "native-frame iOS audio requires unchanged PCM format and zero added latency",
            ));
        }
        // AVAudioUnitTimePitch owns playback rate in this route.
        processor.set_playback_policy(1.0, VESPER_PITCH_PRESERVE, epoch)?;
        Ok(Self {
            processor,
            channels,
            sample_rate,
            block_frames: usize::from(output.max_block_frames.min(MAX_BLOCK_FRAMES)),
            epoch,
        })
    }

    fn process(&mut self, samples: &mut [f32], pts_us: i64) -> PluginResult<()> {
        let channels = usize::from(self.channels.get());
        if samples.is_empty()
            || !samples.len().is_multiple_of(channels)
            || samples.len() / channels > usize::from(MAX_BLOCK_FRAMES)
        {
            return Err(PluginError::Contract("invalid native-frame iOS PCM block"));
        }
        for (index, block) in samples.chunks_mut(self.block_frames * channels).enumerate() {
            let offset_us =
                (index * self.block_frames) as i64 * 1_000_000 / i64::from(self.sample_rate);
            let time = AudioTime {
                pts_us: if pts_us == i64::MIN {
                    pts_us
                } else {
                    pts_us.saturating_add(offset_us)
                },
                rate_epoch: self.epoch,
            };
            self.processor.process_block(AudioBlock::InPlace {
                audio: AudioBufferMut::interleaved(self.channels, block)?,
                time,
            })?;
        }
        Ok(())
    }
}

/// Opens a fixed-block processor from a single verified bundle registry reference.
///
/// # Safety
/// `binding` must point to `length` readable UTF-8 bytes for the call.
pub(crate) unsafe fn open(
    binding: *const c_char,
    length: usize,
    sample_rate: u32,
    channels: u16,
    epoch: u64,
) -> u64 {
    catch_unwind(|| {
        if binding.is_null()
            || length == 0
            || length > MAX_MOBILE_NATIVE_PLUGIN_ARTIFACTS_JSON_BYTES
        {
            return None;
        }
        // SAFETY: the caller owns the readable UTF-8 range for this synchronous call.
        let bytes = unsafe { std::slice::from_raw_parts(binding.cast::<u8>(), length) };
        let artifacts =
            parse_mobile_native_plugin_artifacts_json(std::str::from_utf8(bytes).ok()?).ok()?;
        let [artifact] = artifacts.as_slice() else {
            return None;
        };
        let admitted = artifact.loader_artifact().ok()?;
        let plugin = admitted.native_plugin()?;
        if artifact
            .reference
            .capability_instance_id()
            .is_some_and(|instance| instance.strip_suffix(".audio") != Some(plugin.id()))
        {
            return None;
        }
        let factory = plugin.audio_processor(NATIVE_CALL_BUDGET).ok()?;
        let session = FixedBlockSession::open(
            factory.as_ref(),
            sample_rate,
            NonZeroU16::new(channels)?,
            epoch,
        )
        .ok()?;
        let mut sessions = SESSIONS.lock().unwrap_or_else(|err| err.into_inner());
        if sessions.len() >= MAX_SESSIONS {
            return None;
        }
        Some(sessions.insert(Arc::new(Mutex::new(Some(session)))))
    })
    .ok()
    .flatten()
    .unwrap_or(0)
}

/// Processes one writable, interleaved PCM block in place.
///
/// # Safety
/// `samples` must point to `sample_count` initialized floats that are writable
/// and exclusively borrowed until this call returns.
pub(crate) unsafe fn process(
    handle: u64,
    samples: *mut f32,
    sample_count: usize,
    pts_us: i64,
) -> bool {
    catch_unwind(AssertUnwindSafe(|| {
        if samples.is_null()
            || !samples.is_aligned()
            || sample_count == 0
            || sample_count > usize::from(MAX_BLOCK_FRAMES) * usize::from(MAX_CHANNELS)
        {
            return false;
        }
        let slot = SESSIONS
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .get(handle)
            .cloned();
        let Some(slot) = slot else { return false };
        let Some(mut session) = slot.lock().unwrap_or_else(|err| err.into_inner()).take() else {
            return false;
        };
        // SAFETY: the caller exclusively lends the initialized range, and the
        // pointer alignment and maximum length were checked above.
        let samples = unsafe { std::slice::from_raw_parts_mut(samples, sample_count) };
        let result = session.process(samples, pts_us);
        if result.is_ok() {
            *slot.lock().unwrap_or_else(|err| err.into_inner()) = Some(session);
        }
        result.is_ok()
    }))
    .unwrap_or(false)
}

/// Retires a handle. A synchronous call already in flight retains its own slot.
pub(crate) fn close(handle: u64) {
    let _ = catch_unwind(|| {
        let retired = SESSIONS
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .remove(handle);
        drop(retired);
    });
}
