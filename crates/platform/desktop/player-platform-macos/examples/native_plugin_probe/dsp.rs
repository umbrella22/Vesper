pub type StereoFrame = [f32; 2];

pub enum Block<'a> {
    InPlace(&'a mut [StereoFrame]),
    Separate(&'a [StereoFrame], &'a mut [StereoFrame]),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DspError {
    BlockLength,
    InputAfterEos,
    PluginStatus(u32),
    InvalidProgress,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Push {
    Consumed(usize),
    Backpressure,
    EndAccepted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pull {
    Produced(usize),
    NeedInput,
    Eof,
}

pub trait AudioProcessor {
    fn process_block(&mut self, block: Block<'_>) -> Result<(), DspError>;
    fn push(&mut self, input: Option<&[StereoFrame]>) -> Result<Push, DspError>;
    fn pull(&mut self, output: &mut [StereoFrame]) -> Result<Pull, DspError>;
    fn flush(&mut self) -> Result<(), DspError>;
}

// Smaller than a 10 ms block at 48 kHz so the probe exercises partial
// consumption. One frame of lookahead makes dropping the EOS tail observable.
const FIFO_FRAMES: usize = 256;

pub struct Gain {
    gain: f32,
    fifo: [StereoFrame; FIFO_FRAMES],
    head: usize,
    len: usize,
    draining: bool,
}

impl Gain {
    pub fn new(gain: f32) -> Self {
        Self {
            gain,
            fifo: [[0.0; 2]; FIFO_FRAMES],
            head: 0,
            len: 0,
            draining: false,
        }
    }
}

fn scale([left, right]: StereoFrame, gain: f32) -> StereoFrame {
    [left * gain, right * gain]
}

impl AudioProcessor for Gain {
    fn process_block(&mut self, block: Block<'_>) -> Result<(), DspError> {
        match block {
            Block::InPlace(frames) => {
                for frame in frames {
                    *frame = scale(*frame, self.gain);
                }
            }
            Block::Separate(input, output) => {
                if input.len() != output.len() {
                    return Err(DspError::BlockLength);
                }
                for (input, output) in input.iter().zip(output) {
                    *output = scale(*input, self.gain);
                }
            }
        }
        Ok(())
    }

    fn push(&mut self, input: Option<&[StereoFrame]>) -> Result<Push, DspError> {
        let Some(input) = input else {
            self.draining = true;
            return Ok(Push::EndAccepted);
        };
        if self.draining {
            return Err(DspError::InputAfterEos);
        }
        if input.is_empty() {
            return Err(DspError::BlockLength);
        }
        let count = input.len().min(FIFO_FRAMES - self.len);
        if count == 0 {
            return Ok(Push::Backpressure);
        }
        for &frame in &input[..count] {
            self.fifo[(self.head + self.len) % FIFO_FRAMES] = frame;
            self.len += 1;
        }
        Ok(Push::Consumed(count))
    }

    fn pull(&mut self, output: &mut [StereoFrame]) -> Result<Pull, DspError> {
        if output.is_empty() {
            return Err(DspError::BlockLength);
        }
        let lookahead = usize::from(!self.draining);
        let count = output.len().min(self.len.saturating_sub(lookahead));
        if count == 0 {
            return Ok(if self.draining {
                Pull::Eof
            } else {
                Pull::NeedInput
            });
        }
        for frame in &mut output[..count] {
            *frame = scale(self.fifo[self.head], self.gain);
            self.head = (self.head + 1) % FIFO_FRAMES;
            self.len -= 1;
        }
        Ok(Pull::Produced(count))
    }

    fn flush(&mut self) -> Result<(), DspError> {
        self.head = 0;
        self.len = 0;
        self.draining = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writable_and_read_only_input_produce_the_same_stereo_block() {
        let mut gain = Gain::new(0.5);
        let input = [[-1.0, 0.5], [0.125, -0.75]];
        let mut in_place = input;
        let mut separate = [[0.0; 2]; 2];
        assert_eq!(gain.process_block(Block::InPlace(&mut in_place)), Ok(()));
        assert_eq!(
            gain.process_block(Block::Separate(&input, &mut separate)),
            Ok(())
        );
        assert_eq!(in_place, [[-0.5, 0.25], [0.0625, -0.375]]);
        assert_eq!(in_place, separate);
        assert_eq!(input, [[-1.0, 0.5], [0.125, -0.75]]);
    }

    #[test]
    fn push_reports_partial_consumption_and_eos_releases_the_last_frame() {
        let mut gain = Gain::new(0.5);
        let mut input = [[0.0; 2]; FIFO_FRAMES + 1];
        input[FIFO_FRAMES - 1] = [0.5, -1.0];
        input[FIFO_FRAMES] = [-0.5, 1.0];
        let mut output = [[0.0; 2]; FIFO_FRAMES];
        assert_eq!(gain.push(Some(&input)), Ok(Push::Consumed(FIFO_FRAMES)));
        assert_eq!(
            gain.push(Some(&input[FIFO_FRAMES..])),
            Ok(Push::Backpressure)
        );
        assert_eq!(gain.pull(&mut output), Ok(Pull::Produced(FIFO_FRAMES - 1)));
        assert_eq!(gain.pull(&mut output), Ok(Pull::NeedInput));
        assert_eq!(
            gain.push(Some(&input[FIFO_FRAMES..])),
            Ok(Push::Consumed(1))
        );
        assert_eq!(gain.push(None), Ok(Push::EndAccepted));
        assert_eq!(gain.pull(&mut output), Ok(Pull::Produced(2)));
        assert_eq!(&output[..2], &[[0.25, -0.5], [-0.25, 0.5]]);
        assert_eq!(gain.pull(&mut output), Ok(Pull::Eof));
        assert_eq!(gain.push(Some(&input)), Err(DspError::InputAfterEos));
        assert_eq!(gain.flush(), Ok(()));
        assert_eq!(gain.push(Some(&input[..1])), Ok(Push::Consumed(1)));
    }
}
