use crate::{HandleRegistry, lock_or_recover, run_jni_entry};
use jni::EnvUnowned;
use jni::errors::{Result as JniResult, ThrowRuntimeExAndDefault};
use jni::objects::{JByteBuffer, JClass, JString};
use jni::sys::{jboolean, jint, jlong};
use player_plugin::author::*;
use player_plugin_abi::{VESPER_PITCH_PRESERVE, VESPER_SAMPLE_F32_INTERLEAVED};
use player_plugin_loader::native::PluginRegistry;
use std::mem::size_of;
use std::num::NonZeroU16;
use std::sync::{Arc, LazyLock, Mutex};

const REFERENCE_GAIN_ID: &str = "io.github.umbrella22.vesper.media3-gain";
const MAX_FRAMES: u16 = 1024;
const MAX_CHANNELS: u16 = 8;
const MAX_SESSIONS: usize = 32;

static FACTORIES: LazyLock<PluginRegistry> = LazyLock::new(|| {
    let mut registry = PluginRegistry::default();
    let _ = registry.register_static_audio_processor(REFERENCE_GAIN_ID, Arc::new(ReferenceGain));
    registry
});

type SessionSlot = Arc<Mutex<Option<FixedBlockSession>>>;
static SESSIONS: LazyLock<Mutex<HandleRegistry<SessionSlot>>> =
    LazyLock::new(|| Mutex::new(HandleRegistry::default()));

struct FixedBlockSession {
    channels: NonZeroU16,
    processor: Box<dyn AudioProcessor>,
    scratch: Vec<f32>,
    frame_limit: usize,
}

impl FixedBlockSession {
    fn open(
        factory: &dyn AudioProcessorFactory,
        channels: NonZeroU16,
        rate: u32,
    ) -> PluginResult<Self> {
        if !factory.capabilities().fixed_block || channels.get() > MAX_CHANNELS || rate == 0 {
            return Err(PluginError::Contract(
                "Media3 requires fixed-block interleaved audio",
            ));
        }
        let input = VesperAudioFormat {
            struct_size: size_of::<VesperAudioFormat>() as u32,
            format_id: 1,
            sample_format: VESPER_SAMPLE_F32_INTERLEAVED,
            sample_rate: rate,
            channels: channels.get(),
            channel_layout: 0,
            max_block_frames: MAX_FRAMES,
        };
        let OpenAudioProcessor {
            output_format: output,
            mut processor,
        } = factory.open(input, &[])?;
        if !processor.capabilities().fixed_block
            || output.format_id != input.format_id
            || output.sample_format != input.sample_format
            || output.sample_rate != rate
            || output.channels != channels.get()
            || output.channel_layout != input.channel_layout
            || output.max_block_frames == 0
            || processor.latency_frames() != 0
        {
            return Err(PluginError::Contract(
                "Media3 requires unchanged PCM layout and zero added latency",
            ));
        }
        processor.set_playback_policy(1.0, VESPER_PITCH_PRESERVE, 1)?;
        Ok(Self {
            channels,
            processor,
            scratch: vec![0.0; usize::from(MAX_FRAMES) * usize::from(channels.get())],
            frame_limit: usize::from(output.max_block_frames.min(MAX_FRAMES)),
        })
    }

    fn process(&mut self, input: &[u8], output: &mut [u8]) -> PluginResult<()> {
        let channels = usize::from(self.channels.get());
        let frame_bytes = channels * size_of::<i16>();
        if input.len() != output.len()
            || input.is_empty()
            || !input.len().is_multiple_of(frame_bytes)
            || input.len() / frame_bytes > usize::from(MAX_FRAMES)
        {
            return Err(PluginError::Contract("invalid Media3 PCM block"));
        }
        let sample_count = input.len() / size_of::<i16>();
        let pcm = &mut self.scratch[..sample_count];
        for (src, dst) in input.chunks_exact(2).zip(pcm.iter_mut()) {
            *dst = f32::from(i16::from_ne_bytes([src[0], src[1]])) / 32768.0;
        }
        for block in pcm.chunks_mut(self.frame_limit * channels) {
            self.processor.process_block(AudioBlock::InPlace {
                audio: AudioBufferMut::interleaved(self.channels, block)?,
                // Media3 owns media time and playback rate in this route.
                time: AudioTime {
                    pts_us: i64::MIN,
                    rate_epoch: 1,
                },
            })?;
        }
        for (src, dst) in pcm.iter().zip(output.chunks_exact_mut(2)) {
            let sample = (*src * 32768.0).round() as i16;
            dst.copy_from_slice(&sample.to_ne_bytes());
        }
        Ok(())
    }
}

struct ReferenceGain;

const FIXED_BLOCK: AudioProcessorCapabilities = AudioProcessorCapabilities {
    fixed_block: true,
    buffered: false,
};

impl AudioProcessorFactory for ReferenceGain {
    fn capabilities(&self) -> AudioProcessorCapabilities {
        FIXED_BLOCK
    }
    fn capabilities_json(&self) -> &str {
        r#"{"fixedBlock":true,"buffered":false}"#
    }
    fn open(&self, input: VesperAudioFormat, _params: &[u8]) -> PluginResult<OpenAudioProcessor> {
        Ok(OpenAudioProcessor {
            output_format: input,
            processor: Box::new(Self),
        })
    }
}

impl AudioProcessor for ReferenceGain {
    fn capabilities(&self) -> AudioProcessorCapabilities {
        FIXED_BLOCK
    }
    fn set_playback_policy(
        &mut self,
        rate: f32,
        _pitch_mode: u32,
        _epoch: u64,
    ) -> PluginResult<()> {
        if rate == 1.0 {
            Ok(())
        } else {
            Err(PluginError::Contract("gain DSP does not own playback rate"))
        }
    }
    fn process_block(&mut self, block: AudioBlock<'_>) -> PluginResult<()> {
        match block {
            AudioBlock::InPlace { mut audio, .. } => {
                for sample in audio.samples() {
                    *sample *= 0.5;
                }
            }
            AudioBlock::Split { input, mut output } => {
                for (input, output) in input.samples().iter().zip(output.samples()) {
                    *output = *input * 0.5;
                }
            }
        }
        Ok(())
    }
    fn latency_frames(&self) -> u32 {
        0
    }
    fn flush(&mut self) -> PluginResult<()> {
        Ok(())
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_umbrella22_vesper_player_android_VesperMedia3DspJni_open(
    mut env: EnvUnowned<'_>,
    _class: JClass<'_>,
    id: JString<'_>,
    rate: jint,
    channels: jint,
) -> jlong {
    run_jni_entry(&mut env, |env| {
        env.with_env(|env| -> JniResult<jlong> {
            let id = id.try_to_string(env)?;
            let (Some(factory), Ok(rate), Some(channels)) = (
                FACTORIES.audio_processor(&id),
                u32::try_from(rate),
                u16::try_from(channels).ok().and_then(NonZeroU16::new),
            ) else {
                return Ok(0);
            };
            let Ok(session) = FixedBlockSession::open(factory.as_ref(), channels, rate) else {
                return Ok(0);
            };
            let mut sessions = lock_or_recover(&SESSIONS);
            if sessions.slots.len() - sessions.free_slots.len() >= MAX_SESSIONS {
                return Ok(0);
            }
            Ok(sessions.insert(Arc::new(Mutex::new(Some(session)))))
        })
        .resolve::<ThrowRuntimeExAndDefault>()
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_umbrella22_vesper_player_android_VesperMedia3DspJni_process(
    mut env: EnvUnowned<'_>,
    _class: JClass<'_>,
    handle: jlong,
    input: JByteBuffer<'_>,
    start: jint,
    output: JByteBuffer<'_>,
    length: jint,
) -> jboolean {
    run_jni_entry(&mut env, |env| {
        env.with_env(|env| -> JniResult<jboolean> {
            let (Ok(start), Ok(length)) = (usize::try_from(start), usize::try_from(length)) else {
                return Ok(false);
            };
            let Some(end) = start.checked_add(length) else {
                return Ok(false);
            };
            if length == 0
                || end > env.get_direct_buffer_capacity(&input)?
                || length > env.get_direct_buffer_capacity(&output)?
            {
                return Ok(false);
            }
            let input_ptr = env.get_direct_buffer_address(&input)?.wrapping_add(start);
            let output_ptr = env.get_direct_buffer_address(&output)?;
            let disjoint = input_ptr
                .addr()
                .checked_add(length)
                .is_some_and(|end| end <= output_ptr.addr())
                || output_ptr
                    .addr()
                    .checked_add(length)
                    .is_some_and(|end| end <= input_ptr.addr());
            if !disjoint || input_ptr.is_null() || output_ptr.is_null() {
                return Ok(false);
            }
            let slot = lock_or_recover(&SESSIONS).get(handle).cloned();
            let Some(slot) = slot else {
                return Ok(false);
            };
            let Some(mut processor) = lock_or_recover(&slot).take() else {
                return Ok(false);
            };
            // SAFETY: the bridge passes a readable caller buffer and its own
            // writable buffer. Their direct addresses and disjoint bounds were
            // checked. Both local references outlive the synchronous call.
            let input = unsafe { std::slice::from_raw_parts(input_ptr, length) };
            // SAFETY: the bridge owns this writable output and it cannot alias input.
            let output = unsafe { std::slice::from_raw_parts_mut(output_ptr, length) };
            let success = processor.process(input, output).is_ok();
            if success {
                *lock_or_recover(&slot) = Some(processor);
            }
            Ok(success)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_umbrella22_vesper_player_android_VesperMedia3DspJni_close(
    mut env: EnvUnowned<'_>,
    _class: JClass<'_>,
    handle: jlong,
) {
    run_jni_entry(&mut env, |_| {
        let removed = lock_or_recover(&SESSIONS).remove(handle);
        drop(removed);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reference_processor_writes_a_separate_buffer_and_preserves_pcm_layout() {
        let channels = NonZeroU16::new(2).expect("stereo");
        let mut session = FixedBlockSession::open(&ReferenceGain, channels, 48_000).expect("gain");
        let input: Vec<_> = [16000_i16, -12000, 8000, -4000]
            .into_iter()
            .flat_map(i16::to_ne_bytes)
            .collect();
        let original = input.clone();
        let mut output = [0_u8; 8];
        session.process(&input, &mut output).expect("PCM DSP");
        assert_eq!(input, original);
        let expected: Vec<_> = [8000_i16, -6000, 4000, -2000]
            .into_iter()
            .flat_map(i16::to_ne_bytes)
            .collect();
        assert_eq!(output.as_slice(), expected);
    }
}
