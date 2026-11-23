use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use crate::{
    dsp::{AudioProcessor, Block, StereoFrame},
    dynamic::DynamicGain,
    metrics,
};
use anyhow::{Context, Result, ensure};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_queue::ArrayQueue;

const RATE: u32 = 48_000;
const BLOCK: usize = 480;
const PREBUFFER: usize = 8 * BLOCK;
const MAX_PCM_BYTES: u64 = 5 * RATE as u64 * 2 * 4;

pub fn verify(gain: &Path, pcm: &Path) -> Result<()> {
    ensure!(
        std::fs::metadata(pcm)?.len() <= MAX_PCM_BYTES,
        "PCM fixture is longer than five seconds"
    );
    let bytes = std::fs::read(pcm)?;
    let (stereo, trailing) = bytes.as_chunks::<8>();
    ensure!(
        trailing.is_empty(),
        "PCM fixture is missing a stereo sample"
    );
    let frames = stereo.iter().map(decode).collect::<Vec<_>>();
    ensure!(
        frames.len() >= PREBUFFER,
        "PCM fixture is shorter than the initial queue"
    );
    // SAFETY: the runner builds this trusted standalone fixture.
    let mut processor = unsafe { DynamicGain::open(gain, 0.5) }?;
    run("native_cpp", &frames, |block| {
        processor
            .process_block(Block::InPlace(block))
            .map_err(|e| anyhow::anyhow!("{e:?}"))
    })
}

fn run(
    name: &str,
    input: &[StereoFrame],
    mut process: impl FnMut(&mut [StereoFrame]) -> Result<()>,
) -> Result<()> {
    let device = cpal::default_host()
        .default_output_device()
        .context("no CPAL output device")?;
    let config = device
        .supported_output_configs()?
        .find(|config| {
            config.channels() == 2
                && config.sample_format() == cpal::SampleFormat::F32
                && (config.min_sample_rate()..=config.max_sample_rate()).contains(&RATE)
        })
        .context("output device lacks 48 kHz stereo F32 support")?
        .with_sample_rate(RATE)
        .config();
    let queue = Arc::new(ArrayQueue::<StereoFrame>::new(PREBUFFER));
    let from_queue = queue.clone();
    let counts = Arc::new(Counts::default());
    let out_counts = counts.clone();
    let error_counts = counts.clone();
    let total = input.len();
    let mut delivered = 0;
    let stream = device.build_output_stream(
        config,
        move |output: &mut [f32], _| {
            // Exercise the real callback cadence without sending audible samples.
            output.fill(0.0);
            let to_write = (output.len() / 2).min(total - delivered);
            let mut missing = 0;
            for _ in 0..to_write {
                if from_queue.pop().is_some() {
                    delivered += 1;
                } else {
                    missing += 1;
                }
            }
            if missing > 0 {
                out_counts.underruns.fetch_add(1, Ordering::Relaxed);
                out_counts
                    .missing_frames
                    .fetch_add(missing, Ordering::Relaxed);
            }
            if delivered == total {
                out_counts.done.store(true, Ordering::Release);
            }
        },
        move |_| error_counts.failed.store(true, Ordering::Release),
        None,
    )?;

    let deadline = Instant::now() + Duration::from_secs(10);
    let mut allocations = 0;
    let mut blocks = 0;
    for original in input.chunks(BLOCK) {
        while queue.capacity() - queue.len() < original.len() {
            ensure_active(&counts, deadline)?;
            thread::sleep(Duration::from_millis(1));
        }
        let mut scratch = [[0.0; 2]; BLOCK];
        let block = &mut scratch[..original.len()];
        block.copy_from_slice(original);
        let (result, _, allocs) = metrics::measure(|| process(block));
        result?;
        ensure!(
            block.iter().zip(original).all(|(output, input)| output
                .iter()
                .zip(input)
                .all(|(out, src)| (out - src * 0.5).abs() <= 1e-7)),
            "{name} gain output mismatch"
        );
        allocations += allocs;
        for frame in block {
            ensure!(
                queue.push(*frame).is_ok(),
                "audio producer exceeded its queue allowance"
            );
        }
        blocks += 1;
        if blocks == PREBUFFER / BLOCK {
            stream.play()?;
        }
    }
    while !counts.done.load(Ordering::Acquire) {
        ensure_active(&counts, deadline)?;
        thread::sleep(Duration::from_millis(1));
    }
    drop(stream);
    ensure!(
        !counts.failed.load(Ordering::Acquire),
        "CPAL reported a stream error"
    );
    println!(
        "audio_device mode={name} rate_hz={RATE} frames={total} underrun_callbacks={} underrun_frames={} rust_host_allocs_per_block={:.3}",
        counts.underruns.load(Ordering::Relaxed),
        counts.missing_frames.load(Ordering::Relaxed),
        allocations as f64 / blocks as f64
    );
    Ok(())
}

#[derive(Default)]
struct Counts {
    done: AtomicBool,
    failed: AtomicBool,
    underruns: AtomicU32,
    missing_frames: AtomicU32,
}

fn ensure_active(counts: &Counts, deadline: Instant) -> Result<()> {
    ensure!(
        Instant::now() < deadline,
        "CPAL playback exceeded its deadline"
    );
    ensure!(
        !counts.failed.load(Ordering::Acquire),
        "CPAL reported a stream error"
    );
    Ok(())
}

fn decode(bytes: &[u8; 8]) -> StereoFrame {
    let [a, b, c, d, e, f, g, h] = *bytes;
    [
        f32::from_le_bytes([a, b, c, d]),
        f32::from_le_bytes([e, f, g, h]),
    ]
}
