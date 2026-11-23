use std::mem::size_of;
use std::num::{NonZeroU16, NonZeroU64};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use player_plugin::author::*;
use player_plugin_abi::*;
use player_plugin_loader::native::{NativePlugin, PluginRegistry};

const MODES: AudioProcessorCapabilities = AudioProcessorCapabilities {
    fixed_block: true,
    buffered: false,
};
const FORMAT: VesperAudioFormat = VesperAudioFormat {
    struct_size: size_of::<VesperAudioFormat>() as u32,
    sample_format: VESPER_SAMPLE_F32_INTERLEAVED,
    format_id: 1,
    sample_rate: 48_000,
    channels: 2,
    max_block_frames: 2048,
    channel_layout: 3,
};
const TIME: AudioTime = AudioTime {
    pts_us: -6000,
    rate_epoch: 1,
};

struct GainFactory;
struct Gain;

impl AudioProcessorFactory for GainFactory {
    fn capabilities(&self) -> AudioProcessorCapabilities {
        MODES
    }
    fn capabilities_json(&self) -> &str {
        r#"{"fixedBlock":true}"#
    }
    fn open(&self, input: VesperAudioFormat, _params: &[u8]) -> PluginResult<OpenAudioProcessor> {
        Ok(OpenAudioProcessor {
            output_format: input,
            processor: Box::new(Gain),
        })
    }
}

impl AudioProcessor for Gain {
    fn capabilities(&self) -> AudioProcessorCapabilities {
        MODES
    }
    fn set_playback_policy(&mut self, _: f32, _: u32, _: u64) -> PluginResult<()> {
        Ok(())
    }
    fn set_param(&mut self, key: &[u8], _: &[u8]) -> PluginResult<()> {
        match key {
            b"panic" => panic!("test panic in Rust plugin"),
            b"unknown" => Err(PluginError::Status(0xf00d_f00d)),
            _ => Ok(()),
        }
    }
    fn process_block(&mut self, block: AudioBlock<'_>) -> PluginResult<()> {
        match block {
            AudioBlock::InPlace { mut audio, .. } => {
                for value in audio.samples() {
                    *value *= 0.5;
                }
            }
            AudioBlock::Split { input, mut output } => {
                for (out, input) in output.samples().iter_mut().zip(input.samples()) {
                    *out = *input * 0.5;
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

#[player_plugin::export]
fn gain() -> Plugin {
    Plugin::new("io.github.vesper.gain", "Gain fixture").with_audio_processor(GainFactory)
}

fn channel_count() -> NonZeroU16 {
    NonZeroU16::new(2).expect("stereo fixture")
}

fn assert_gain(processor: &mut dyn AudioProcessor) {
    let input = [0.5, -1.0, 0.0, 1.0, 0.875, -0.25];
    let expected = input.map(|value| value * 0.5);
    // Output is larger than the input, so the unused frames stay intact.
    let mut output = [17.0; 8];
    processor
        .process_block(AudioBlock::Split {
            input: AudioBufferView::interleaved(channel_count(), &input, TIME).unwrap(),
            output: AudioBufferMut::interleaved(channel_count(), &mut output).unwrap(),
        })
        .unwrap();
    assert_eq!(output[..input.len()], expected);
    assert_eq!(output[input.len()..], [17.0, 17.0]);
    assert_eq!(input[1], -1.0);
    let mut in_place = input;
    processor
        .process_block(AudioBlock::InPlace {
            audio: AudioBufferMut::interleaved(channel_count(), &mut in_place).unwrap(),
            time: TIME,
        })
        .unwrap();
    assert_eq!(in_place, expected);
}

#[test]
fn static_and_exported_rust_share_audio_views() {
    let mut registry = PluginRegistry::default();
    registry
        .register_static_audio_processor("io.github.vesper.gain", Arc::new(GainFactory))
        .unwrap();
    let mut static_processor = registry
        .audio_processor("io.github.vesper.gain")
        .unwrap()
        .open(FORMAT, &[])
        .unwrap()
        .processor;
    // SAFETY: the entry was generated from the checked Rust factory in this test.
    let plugin = unsafe { NativePlugin::from_entry(vesper_native_plugin_entry) }.unwrap();
    let factory = plugin.audio_processor(Duration::from_secs(1)).unwrap();
    let mut exported = factory.open(FORMAT, &[]).unwrap().processor;
    drop(plugin);
    drop(factory);
    assert_gain(static_processor.as_mut());
    assert_gain(exported.as_mut());
    assert_eq!(
        exported.set_param(b"unknown", b""),
        Err(PluginError::Status(0xf00d_f00d))
    );
    assert_eq!(
        exported.set_param(b"panic", b""),
        Err(PluginError::Status(VESPER_ERR_PANIC))
    );
    assert_eq!(exported.flush(), Err(PluginError::CircuitOpen));
}

#[test]
fn slot_generations_and_frame_ownership_survive_a_session() {
    let mut slots = LeaseSlots::default();
    let ids: Vec<_> = (0..VESPER_MAX_LEASES_PER_SESSION)
        .map(|value| slots.insert(value).unwrap())
        .collect();
    assert_eq!(
        slots.insert(1024),
        Err(PluginError::Status(VESPER_ERR_EXHAUSTED))
    );
    let old = ids[3];
    assert_eq!(slots.release(old), Some(3));
    let new = slots.insert(4).unwrap();
    assert_ne!(new, old);
    assert_eq!(slots.release(old), None);
    assert_eq!(slots.get(new), Some(&4));
    slots.clear();
    assert_eq!(slots.release(new), None);

    struct Release(Arc<AtomicUsize>);
    impl ReleaseVideoFrame for Release {
        fn release(&self, lease: NonZeroU64, disposition: FrameDisposition) {
            assert_eq!(lease.get(), 41);
            assert_eq!(disposition, FrameDisposition::Rendered);
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }
    let count = Arc::new(AtomicUsize::new(0));
    let context = Arc::new(Release(Arc::clone(&count)));
    let frame = OwnedVideoFrame::new(
        VideoFrame {
            format_id: 1,
            pts_us: 0,
            duration_us: 16_667,
            flags: 0,
            handle_kind: VESPER_HANDLE_MTL_TEXTURE,
            plane_count: 1,
            handles: [1, 0, 0],
            strides: [0; 3],
            sync_kind: VESPER_SYNC_NONE,
            sync_handle: 0,
            sync_value: 0,
        },
        NonZeroU64::new(41).unwrap(),
        context.clone(),
    );
    drop(context);
    std::thread::spawn(move || frame.complete(FrameDisposition::Rendered))
        .join()
        .unwrap();
    assert_eq!(count.load(Ordering::Relaxed), 1);
}

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
#[test]
#[ignore = "requires VESPER_AUDIO_REFERENCE pointing to the separately compiled C++ reference"]
fn cpp_reference_fixed_block_and_wsola_drain() {
    let library =
        std::env::var_os("VESPER_AUDIO_REFERENCE").expect("path to the trusted reference library");
    // SAFETY: this test explicitly selects the local C++ reference built from the checked header.
    let plugin = unsafe { NativePlugin::load(library) }.unwrap();
    assert_eq!(plugin.interface_ids(), &[VESPER_AUDIO_PROCESSOR_ID]);
    let factory = plugin.audio_processor(Duration::from_secs(1)).unwrap();
    let mut processor = factory.open(FORMAT, &[]).unwrap().processor;
    processor.set_param(b"gain", b"0.5").unwrap();
    assert_gain(processor.as_mut());

    let input: Vec<f32> = (0..2048)
        .flat_map(|frame| {
            let x = (frame as f32 * 2.0 * std::f32::consts::PI * 440.0 / 48_000.0).sin();
            [x, -x]
        })
        .collect();
    let mut output = [0.0; 640];
    let mut phase_totals = [0; 3];
    for (index, rate) in [1.0, 2.0, 0.5].into_iter().enumerate() {
        let rate_epoch = index as u64 + 1;
        let pts_us = index as i64 * 50_000;
        processor
            .set_playback_policy(rate, VESPER_PITCH_PRESERVE, rate_epoch)
            .unwrap();
        let view =
            AudioBufferView::interleaved(channel_count(), &input, AudioTime { pts_us, rate_epoch })
                .unwrap();
        let mut accepted = false;
        for _ in 0..64 {
            if !accepted {
                accepted = match processor.push(Some(view)).unwrap() {
                    AudioPush::Consumed(2048) => true,
                    AudioPush::Backpressure => false,
                    result => panic!("unexpected push: {result:?}"),
                };
            }
            match processor
                .pull(AudioBufferMut::interleaved(channel_count(), &mut output).unwrap())
                .unwrap()
            {
                AudioPull::Produced { frames, time } => {
                    let epoch = (time.rate_epoch - 1) as usize;
                    assert!(epoch <= index);
                    let out_rate = [1.0, 2.0, 0.5][epoch];
                    let prior = phase_totals[epoch];
                    let expected = epoch as i64 * 50_000
                        + (prior as f64 * out_rate * 1_000_000.0 / 48_000.0) as i64;
                    assert_eq!(time.pts_us, expected);
                    phase_totals[epoch] += frames;
                    assert!(
                        output
                            .iter()
                            .take(frames as usize * 2)
                            .all(|sample| sample.is_finite())
                    );
                }
                AudioPull::NeedInput if accepted => break,
                AudioPull::NeedInput | AudioPull::TryAgain => {}
                AudioPull::Eof => panic!("unexpected EOF"),
            }
        }
        assert!(accepted);
    }
    assert_eq!(processor.push(None).unwrap(), AudioPush::EosAccepted);
    let mut eof = false;
    for _ in 0..32 {
        match processor
            .pull(AudioBufferMut::interleaved(channel_count(), &mut output).unwrap())
            .unwrap()
        {
            AudioPull::Produced { frames, time } => {
                assert_eq!(time.rate_epoch, 3);
                phase_totals[2] += frames;
            }
            AudioPull::Eof => {
                eof = true;
                break;
            }
            other => panic!("unexpected drain: {other:?}"),
        }
    }
    assert!(eof);
    assert_eq!(phase_totals, [2048, 1024, 4096]);
    processor.flush().unwrap();
    assert_eq!(
        processor
            .pull(AudioBufferMut::interleaved(channel_count(), &mut output).unwrap())
            .unwrap(),
        AudioPull::NeedInput
    );
}

#[test]
fn linked_entry_rejects_wrong_abi_major_and_truncated_root() {
    extern "C" fn wrong_major() -> *const VesperPluginRoot {
        &const {
            VesperPluginRoot {
                struct_size: size_of::<VesperPluginRoot>() as u32,
                abi_major: 1,
                abi_minor: 0,
                owner: std::ptr::null_mut(),
                plugin_id: VesperBytes {
                    data: std::ptr::null(),
                    len: 0,
                },
                plugin_name: VesperBytes {
                    data: std::ptr::null(),
                    len: 0,
                },
                interface_count: 0,
                interface_at: None,
                query_interface: None,
                destroy_owner: None,
            }
        }
    }
    extern "C" fn truncated() -> *const VesperPluginRoot {
        &const { 4_u64 } as *const u64 as *const VesperPluginRoot
    }
    // SAFETY: the fixtures supply readable, aligned leading fields and contain no callbacks.
    let version = unsafe { NativePlugin::from_entry(wrong_major) };
    assert!(matches!(
        version,
        Err(PluginError::Contract("incompatible plugin ABI major"))
    ));
    // SAFETY: the fixture supplies a readable, aligned size field with no table to adopt.
    let size = unsafe { NativePlugin::from_entry(truncated) };
    assert!(matches!(size, Err(PluginError::Contract(_))));
}
