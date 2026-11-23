use std::path::Path;

use anyhow::{Result, bail, ensure};

use crate::{dsp::*, dynamic::DynamicGain, metrics};

const GAIN: f32 = 0.5;
const BLOCK_FRAMES: usize = 480;
const MAX_STEPS_PER_BLOCK: usize = 32;

pub fn verify(path: &Path) -> Result<()> {
    let mut direct = Gain::new(GAIN);
    // SAFETY: the path is the trusted fixture built by the probe runner.
    let mut dynamic = unsafe { DynamicGain::open(path, GAIN) }?;
    let input = std::array::from_fn::<_, BLOCK_FRAMES, _>(|i| {
        [
            i as f32 / BLOCK_FRAMES as f32,
            -(i as f32 / BLOCK_FRAMES as f32),
        ]
    });
    let expected = input.map(|[l, r]| [l * GAIN, r * GAIN]);
    for (name, processor) in [
        ("rust", &mut direct as &mut dyn AudioProcessor),
        ("cpp", &mut dynamic as &mut dyn AudioProcessor),
    ] {
        let mut in_place = input;
        let mut output = [[0.0; 2]; BLOCK_FRAMES];
        let (processed, _, allocations) =
            metrics::measure(|| processor.process_block(Block::InPlace(&mut in_place)));
        step(processed)?;
        ensure!(
            allocations == 0,
            "{name} block processing allocated in the Rust host"
        );
        step(processor.process_block(Block::Separate(&input, &mut output)))?;
        ensure!(
            in_place == expected && output == expected,
            "block output mismatch"
        );
        ensure!(
            stream(processor, &input)? == expected,
            "stream output or EOS tail mismatch"
        );
        step(processor.flush())?;
        ensure!(
            step(processor.push(Some(&input[..1])))? == Push::Consumed(1),
            "new input rejected after flush"
        );
    }
    verify_rate_switch(&mut dynamic)?;
    println!(
        "audio_static_and_cpp=pass frames={BLOCK_FRAMES} block_in_place=pass block_disjoint=pass eos_tail=pass"
    );
    Ok(())
}

fn stream(
    processor: &mut dyn AudioProcessor,
    input: &[StereoFrame; BLOCK_FRAMES],
) -> Result<[StereoFrame; BLOCK_FRAMES]> {
    let mut output = [[0.0; 2]; BLOCK_FRAMES];
    let mut consumed = 0;
    let mut produced = 0;
    let mut draining = false;
    for _ in 0..MAX_STEPS_PER_BLOCK {
        let before = (consumed, produced, draining);
        if consumed < input.len() {
            match step(processor.push(Some(&input[consumed..])))? {
                Push::Consumed(n) if n > 0 && n <= input.len() - consumed => consumed += n,
                Push::Backpressure => {}
                other => bail!("unexpected input progress {other:?}"),
            }
        } else if !draining {
            ensure!(
                step(processor.push(None))? == Push::EndAccepted,
                "EOS rejected"
            );
            draining = true;
        }
        let mut scratch = [[0.0; 2]; BLOCK_FRAMES];
        match step(processor.pull(&mut scratch))? {
            Pull::Produced(n) if n > 0 && n <= output.len() - produced => {
                output[produced..produced + n].copy_from_slice(&scratch[..n]);
                produced += n;
            }
            Pull::NeedInput if !draining => {}
            Pull::Eof if draining && produced == output.len() => return Ok(output),
            other => bail!("unexpected output progress {other:?}"),
        }
        ensure!(
            before != (consumed, produced, draining),
            "DSP scheduler made no progress"
        );
    }
    bail!("DSP scheduler exceeded its step budget")
}

fn step<T>(result: Result<T, DspError>) -> Result<T> {
    result.map_err(|error| anyhow::anyhow!("DSP error {error:?}"))
}

fn verify_rate_switch(processor: &mut DynamicGain) -> Result<()> {
    // Forty-eight input frames cover 1 ms at 48 kHz at all three speeds.
    // Leave all three rate segments buffered inside the same DSP instance.
    const INPUT_FRAMES: usize = 48;
    const RATE_HZ: f64 = 48_000.0;
    let segments = [(1, 1.0, 48), (2, 2.0, 24), (3, 0.5, 96)];
    let mut input = [[0.0; 2]; INPUT_FRAMES];
    let mut output = [[0.0; 2]; 256];
    let mut buffered = 0;
    step(processor.flush())?;
    for (epoch, rate, frames) in segments {
        step(processor.set_policy(rate, epoch))?;
        ensure!(
            step(processor.latency_frames())? == buffered,
            "switch discarded or retimed already buffered frames"
        );
        input.fill([epoch as f32, -(epoch as f32)]);
        ensure!(
            step(processor.push(Some(&input)))? == Push::Consumed(INPUT_FRAMES),
            "rate segment did not fit in the probe FIFO"
        );
        buffered += frames;
    }
    ensure!(step(processor.latency_frames())? == buffered);

    let mut media_us = 1_000_000_u64;
    for (epoch, rate, frames) in segments {
        let last = epoch == 3;
        let playable = frames as usize - usize::from(last);
        let (pull, output_epoch) = step(processor.pull_segment(&mut output))?;
        ensure!(
            (pull, output_epoch) == (Pull::Produced(playable), epoch),
            "a pull crossed a rate boundary or skipped a segment"
        );
        ensure!(
            output[..playable]
                .iter()
                .all(|&frame| frame == [epoch as f32 * GAIN, -(epoch as f32) * GAIN]),
            "output payload does not match the returned epoch"
        );
        // Same sample-offset/rate mapping as CPAL, with a separate origin for
        // each rate epoch. This uses only the output that has been delivered.
        media_us += ((playable as f64 * f64::from(rate) / RATE_HZ) * 1e6).round() as u64;
        let expected = 1_000_000 + epoch * 1_000 - if last { 10 } else { 0 };
        ensure!(media_us == expected, "media clock jumped at epoch {epoch}");
    }
    ensure!(step(processor.latency_frames())? == 1);

    // A seek discards the one-frame lookahead. Block delivery from older
    // playback generations at the sink (covered by the CPAL timeline test).
    step(processor.flush())?;
    step(processor.set_policy(0.5, 4))?;
    ensure!(
        step(processor.pull_segment(&mut output))? == (Pull::NeedInput, 0),
        "old samples survived the seek flush"
    );
    input.fill([0.25, -0.25]);
    ensure!(step(processor.push(Some(&input)))? == Push::Consumed(INPUT_FRAMES));
    ensure!(step(processor.push(None))? == Push::EndAccepted);
    let (pull, epoch) = step(processor.pull_segment(&mut output))?;
    ensure!((pull, epoch) == (Pull::Produced(96), 4));
    ensure!(output[..96].iter().all(|&frame| frame == [0.125, -0.125]));
    ensure!(step(processor.pull_segment(&mut output))? == (Pull::Eof, 0));
    println!(
        "audio_buffered_rate_switch=pass rates=1.0,2.0,0.5 max_buffered_frames={buffered} pre_seek_media_us={media_us} seek_dropped_tail_frames=1"
    );
    Ok(())
}
