//! Safe media views shared by static plugins and native ABI adapters.

#![deny(unsafe_code)]

use std::fmt;
use std::num::{NonZeroU16, NonZeroU64};
use std::sync::Arc;

use super::*;

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum PluginError {
    Status(VesperStatus),
    StatusWithDetail(VesperStatus, String),
    Contract(&'static str),
    CircuitOpen,
}

impl fmt::Display for PluginError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Status(code) => write!(f, "plugin returned status {code}"),
            Self::StatusWithDetail(code, detail) => {
                write!(f, "plugin returned status {code}: {detail}")
            }
            Self::Contract(message) => write!(f, "plugin contract violation: {message}"),
            Self::CircuitOpen => f.write_str("plugin session circuit is open"),
        }
    }
}

impl std::error::Error for PluginError {}

pub type PluginResult<T> = Result<T, PluginError>;

/// Tracks outstanding ownership in one bounded, exclusively accessed session.
/// Each slot has an independent generation. Reusing one slot cannot revive a
/// lease that was returned before a seek or flush.
pub struct LeaseSlots<T> {
    entries: [Option<T>; VESPER_MAX_LEASES_PER_SESSION],
    generations: [u64; VESPER_MAX_LEASES_PER_SESSION],
    occupied: u64,
}

impl<T> Default for LeaseSlots<T> {
    fn default() -> Self {
        Self {
            entries: std::array::from_fn(|_| None),
            generations: [0; VESPER_MAX_LEASES_PER_SESSION],
            occupied: 0,
        }
    }
}

impl<T> LeaseSlots<T> {
    const SLOT_BITS: u32 = VESPER_MAX_LEASES_PER_SESSION.ilog2();
    const SLOT_MASK: u64 = (1 << Self::SLOT_BITS) - 1;
    const MAX_GENERATION: u64 = u64::MAX >> Self::SLOT_BITS;

    pub fn is_full(&self) -> bool {
        self.occupied == u64::MAX
    }

    pub fn insert(&mut self, value: T) -> PluginResult<NonZeroU64> {
        let index = self.occupied.trailing_ones() as usize;
        let generation = self
            .generations
            .get_mut(index)
            .filter(|current| **current < Self::MAX_GENERATION)
            .ok_or(PluginError::Status(VESPER_ERR_EXHAUSTED))?;
        *generation += 1;
        let lease_id = NonZeroU64::new((*generation << Self::SLOT_BITS) | index as u64)
            .ok_or(PluginError::Status(VESPER_ERR_INTERNAL))?;
        self.entries[index] = Some(value);
        self.occupied |= 1 << index;
        Ok(lease_id)
    }

    pub fn get(&self, lease_id: NonZeroU64) -> Option<&T> {
        let index = self.index(lease_id)?;
        self.entries[index].as_ref()
    }

    pub fn release(&mut self, lease_id: NonZeroU64) -> Option<T> {
        let index = self.index(lease_id)?;
        self.occupied &= !(1 << index);
        self.entries[index].take()
    }

    pub fn clear(&mut self) {
        while self.occupied != 0 {
            let index = self.occupied.trailing_zeros() as usize;
            self.occupied &= !(1 << index);
            self.entries[index] = None;
        }
    }

    fn index(&self, lease_id: NonZeroU64) -> Option<usize> {
        let raw = lease_id.get();
        let index = (raw & Self::SLOT_MASK) as usize;
        (raw >> Self::SLOT_BITS == self.generations[index]).then_some(index)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VideoFrame {
    pub format_id: u64,
    pub pts_us: i64,
    pub duration_us: i64,
    pub flags: u32,
    pub handle_kind: u32,
    pub plane_count: u32,
    pub handles: [u64; 3],
    pub strides: [u32; 3],
    pub sync_kind: u32,
    pub sync_handle: u64,
    pub sync_value: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameDisposition {
    Rendered,
    Dropped,
}

/// A release context follows its leases and receives completions on any thread.
/// Implementations should return resources to the session's bounded pool.
pub trait ReleaseVideoFrame: Send + Sync {
    fn release(&self, lease_id: NonZeroU64, disposition: FrameDisposition);
}

/// The presenter keeps ownership until the GPU finishes all uses of the frame.
/// An ownership transfer moves this value, including across threads.
pub struct OwnedVideoFrame {
    frame: VideoFrame,
    release: Option<(NonZeroU64, Arc<dyn ReleaseVideoFrame>)>,
}

impl OwnedVideoFrame {
    pub fn new(
        frame: VideoFrame,
        lease_id: NonZeroU64,
        release: Arc<dyn ReleaseVideoFrame>,
    ) -> Self {
        Self {
            frame,
            release: Some((lease_id, release)),
        }
    }

    pub fn frame(&self) -> &VideoFrame {
        &self.frame
    }

    pub fn complete(mut self, disposition: FrameDisposition) {
        if let Some((id, release)) = self.release.take() {
            release.release(id, disposition);
        }
    }
}

impl Drop for OwnedVideoFrame {
    fn drop(&mut self) {
        if let Some((id, release)) = self.release.take() {
            release.release(id, FrameDisposition::Dropped);
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct AudioTime {
    pub pts_us: i64,
    pub rate_epoch: u64,
}

#[derive(Debug, Clone, Copy)]
pub struct AudioBufferView<'a> {
    channels: NonZeroU16,
    samples: &'a [f32],
    pub time: AudioTime,
}

impl<'a> AudioBufferView<'a> {
    pub fn interleaved(
        channels: NonZeroU16,
        samples: &'a [f32],
        time: AudioTime,
    ) -> PluginResult<Self> {
        if !samples.len().is_multiple_of(usize::from(channels.get())) {
            return Err(PluginError::Contract(
                "input contains a partial audio frame",
            ));
        }
        Ok(Self {
            channels,
            samples,
            time,
        })
    }

    pub fn samples(self) -> &'a [f32] {
        self.samples
    }

    pub fn channels(self) -> NonZeroU16 {
        self.channels
    }

    pub fn frames(self) -> usize {
        self.samples.len() / usize::from(self.channels.get())
    }
}

pub struct AudioBufferMut<'a> {
    channels: NonZeroU16,
    samples: &'a mut [f32],
}

impl<'a> AudioBufferMut<'a> {
    pub fn interleaved(channels: NonZeroU16, samples: &'a mut [f32]) -> PluginResult<Self> {
        if !samples.len().is_multiple_of(usize::from(channels.get())) {
            return Err(PluginError::Contract(
                "output contains a partial audio frame",
            ));
        }
        Ok(Self { channels, samples })
    }

    pub fn samples(&mut self) -> &mut [f32] {
        self.samples
    }

    pub fn channels(&self) -> NonZeroU16 {
        self.channels
    }

    pub fn frames(&self) -> usize {
        self.samples.len() / usize::from(self.channels.get())
    }
}

/// Fixed-block DSP gets either exclusive PCM or a disjoint read/write pair.
pub enum AudioBlock<'a> {
    InPlace {
        audio: AudioBufferMut<'a>,
        time: AudioTime,
    },
    Split {
        input: AudioBufferView<'a>,
        output: AudioBufferMut<'a>,
    },
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum AudioPush {
    Consumed(u32),
    Backpressure,
    EosAccepted,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum AudioPull {
    Produced { frames: u32, time: AudioTime },
    NeedInput,
    TryAgain,
    Eof,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct AudioProcessorCapabilities {
    pub fixed_block: bool,
    pub buffered: bool,
}

pub struct OpenAudioProcessor {
    pub output_format: VesperAudioFormat,
    pub processor: Box<dyn AudioProcessor>,
}

pub trait AudioProcessorFactory: Send + Sync {
    fn capabilities(&self) -> AudioProcessorCapabilities;
    fn capabilities_json(&self) -> &str;
    fn open(&self, input: VesperAudioFormat, params: &[u8]) -> PluginResult<OpenAudioProcessor>;
}

pub trait AudioProcessor: Send {
    fn capabilities(&self) -> AudioProcessorCapabilities;

    fn set_playback_policy(&mut self, rate: f32, pitch_mode: u32, epoch: u64) -> PluginResult<()>;

    fn set_param(&mut self, _key: &[u8], _value: &[u8]) -> PluginResult<()> {
        Err(PluginError::Status(VESPER_ERR_UNSUPPORTED))
    }

    fn process_block(&mut self, _block: AudioBlock<'_>) -> PluginResult<()> {
        Err(PluginError::Status(VESPER_ERR_UNSUPPORTED))
    }

    fn push(&mut self, _input: Option<AudioBufferView<'_>>) -> PluginResult<AudioPush> {
        Err(PluginError::Status(VESPER_ERR_UNSUPPORTED))
    }

    fn pull(&mut self, _output: AudioBufferMut<'_>) -> PluginResult<AudioPull> {
        Err(PluginError::Status(VESPER_ERR_UNSUPPORTED))
    }

    fn latency_frames(&self) -> u32;
    fn flush(&mut self) -> PluginResult<()>;
}

/// Capabilities compiled into one plugin root. Registration is cold-path work.
pub struct Plugin {
    pub(crate) id: &'static str,
    pub(crate) name: &'static str,
    pub(crate) audio: Option<Arc<dyn AudioProcessorFactory>>,
}

impl Plugin {
    pub fn new(id: &'static str, name: &'static str) -> Self {
        Self {
            id,
            name,
            audio: None,
        }
    }

    pub fn with_audio_processor(mut self, factory: impl AudioProcessorFactory + 'static) -> Self {
        self.audio = Some(Arc::new(factory));
        self
    }
}
