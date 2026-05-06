use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use anyhow::{Context, Result};
use player_model::MediaSource;

use crate::{DecodedVideoFrame, FfmpegBackend, MediaProbe, VideoDecodeInfo};

const PREFETCH_RETRY_INTERVAL: Duration = Duration::from_millis(1);

#[derive(Debug)]
pub enum BufferedFramePoll {
    Ready(DecodedVideoFrame),
    Pending,
    EndOfStream,
}

#[derive(Debug)]
pub struct BufferedVideoSource {
    command_tx: Sender<WorkerCommand>,
    frame_rx: Receiver<WorkerEvent>,
    generation: u64,
    current_generation: Arc<AtomicU64>,
    buffered_frame_count: Arc<AtomicUsize>,
    prefetch_limit: Arc<AtomicUsize>,
    ended: bool,
    worker: Option<JoinHandle<()>>,
}

#[derive(Debug)]
pub struct BufferedVideoSourceBootstrap {
    pub source: BufferedVideoSource,
    pub decode_info: VideoDecodeInfo,
    pub probe: MediaProbe,
}

#[derive(Debug)]
enum WorkerCommand {
    Seek { generation: u64, position: Duration },
    Shutdown,
}

#[derive(Debug)]
enum WorkerEvent {
    Frame {
        generation: u64,
        frame: DecodedVideoFrame,
    },
    EndOfStream {
        generation: u64,
    },
    Error {
        generation: u64,
        message: String,
    },
}

#[derive(Debug)]
struct BufferedVideoSourceInit {
    decode_info: VideoDecodeInfo,
    probe: MediaProbe,
}

impl BufferedVideoSource {
    pub fn new(
        source: MediaSource,
        buffer_capacity: usize,
    ) -> Result<BufferedVideoSourceBootstrap> {
        Self::new_with_interrupt(source, buffer_capacity, None)
    }

    pub fn new_with_interrupt(
        source: MediaSource,
        buffer_capacity: usize,
        interrupt_flag: Option<Arc<AtomicBool>>,
    ) -> Result<BufferedVideoSourceBootstrap> {
        let (command_tx, command_rx) = mpsc::channel();
        let (frame_tx, frame_rx) = mpsc::channel();
        let (init_tx, init_rx) = mpsc::channel();
        let current_generation = Arc::new(AtomicU64::new(0));
        let buffered_frame_count = Arc::new(AtomicUsize::new(0));
        let prefetch_limit = Arc::new(AtomicUsize::new(buffer_capacity.max(1)));
        let worker_generation = current_generation.clone();
        let worker_buffered_frame_count = buffered_frame_count.clone();
        let worker_prefetch_limit = prefetch_limit.clone();
        let worker = thread::Builder::new()
            .name("ffmpeg-video-prefetch".to_owned())
            .spawn(move || {
                worker_loop(
                    source,
                    interrupt_flag,
                    command_rx,
                    frame_tx,
                    init_tx,
                    worker_generation,
                    worker_buffered_frame_count,
                    worker_prefetch_limit,
                )
            })
            .context("failed to spawn video predecode worker")?;
        let init = init_rx
            .recv()
            .context("video predecode worker disconnected before reporting decoder info")??;

        Ok(BufferedVideoSourceBootstrap {
            source: Self {
                command_tx,
                frame_rx,
                generation: 0,
                current_generation,
                buffered_frame_count,
                prefetch_limit,
                ended: false,
                worker: Some(worker),
            },
            decode_info: init.decode_info,
            probe: init.probe,
        })
    }

    pub fn recv_frame(&mut self) -> Result<Option<DecodedVideoFrame>> {
        if self.ended {
            return Ok(None);
        }

        loop {
            let event = self
                .frame_rx
                .recv()
                .context("video predecode worker disconnected")?;
            if let Some(frame) = self.handle_event(event)? {
                return Ok(Some(frame));
            }

            if self.ended {
                return Ok(None);
            }
        }
    }

    pub fn try_recv_frame(&mut self) -> Result<BufferedFramePoll> {
        if self.ended {
            return Ok(BufferedFramePoll::EndOfStream);
        }

        loop {
            match self.frame_rx.try_recv() {
                Ok(event) => {
                    if let Some(frame) = self.handle_event(event)? {
                        return Ok(BufferedFramePoll::Ready(frame));
                    }

                    if self.ended {
                        return Ok(BufferedFramePoll::EndOfStream);
                    }
                }
                Err(TryRecvError::Empty) => return Ok(BufferedFramePoll::Pending),
                Err(TryRecvError::Disconnected) => {
                    anyhow::bail!("video predecode worker disconnected")
                }
            }
        }
    }

    pub fn seek_to(&mut self, position: Duration) -> Result<Option<DecodedVideoFrame>> {
        self.generation = self.generation.wrapping_add(1);
        self.current_generation
            .store(self.generation, Ordering::SeqCst);
        self.buffered_frame_count.store(0, Ordering::SeqCst);
        self.ended = false;
        self.command_tx
            .send(WorkerCommand::Seek {
                generation: self.generation,
                position,
            })
            .context("failed to send seek request to video predecode worker")?;

        self.recv_frame()
    }

    pub fn buffered_frame_count(&self) -> usize {
        self.buffered_frame_count.load(Ordering::SeqCst)
    }

    pub fn set_prefetch_limit(&self, limit: usize) {
        self.prefetch_limit.store(limit.max(1), Ordering::SeqCst);
    }

    fn handle_event(&mut self, event: WorkerEvent) -> Result<Option<DecodedVideoFrame>> {
        match event {
            WorkerEvent::Frame { generation, frame } if generation == self.generation => {
                decrement_buffered_frame_count(&self.buffered_frame_count);
                Ok(Some(frame))
            }
            WorkerEvent::EndOfStream { generation } if generation == self.generation => {
                self.ended = true;
                Ok(None)
            }
            WorkerEvent::Error {
                generation,
                message,
            } if generation == self.generation => Err(anyhow::anyhow!(message)),
            _ => Ok(None),
        }
    }
}

impl Drop for BufferedVideoSource {
    fn drop(&mut self) {
        let _ = self.command_tx.send(WorkerCommand::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn worker_loop(
    source: MediaSource,
    interrupt_flag: Option<Arc<AtomicBool>>,
    command_rx: Receiver<WorkerCommand>,
    frame_tx: Sender<WorkerEvent>,
    init_tx: Sender<Result<BufferedVideoSourceInit>>,
    current_generation: Arc<AtomicU64>,
    buffered_frame_count: Arc<AtomicUsize>,
    prefetch_limit: Arc<AtomicUsize>,
) {
    let media_source = source;
    let backend = match FfmpegBackend::new() {
        Ok(backend) => backend,
        Err(error) => {
            let _ = init_tx.send(Err(anyhow::anyhow!(error.to_string())));
            let _ = frame_tx.send(WorkerEvent::Error {
                generation: 0,
                message: error.to_string(),
            });
            return;
        }
    };
    let mut video_source =
        match backend.open_video_source_with_interrupt(media_source.clone(), interrupt_flag) {
            Ok(source) => source,
            Err(error) => {
                let _ = init_tx.send(Err(anyhow::anyhow!(error.to_string())));
                let _ = frame_tx.send(WorkerEvent::Error {
                    generation: 0,
                    message: error.to_string(),
                });
                return;
            }
        };
    let probe = match video_source.media_probe(&media_source) {
        Ok(probe) => probe,
        Err(error) => {
            let _ = init_tx.send(Err(anyhow::anyhow!(error.to_string())));
            let _ = frame_tx.send(WorkerEvent::Error {
                generation: 0,
                message: error.to_string(),
            });
            return;
        }
    };
    let _ = init_tx.send(Ok(BufferedVideoSourceInit {
        decode_info: video_source.decode_info().clone(),
        probe,
    }));
    let mut generation = 0u64;
    let mut pending_event = None;

    loop {
        match latest_command(&command_rx) {
            Some(WorkerCommand::Shutdown) => break,
            Some(WorkerCommand::Seek {
                generation: new_generation,
                position,
            }) => {
                generation = new_generation;
                pending_event = Some(match video_source.seek_to(position) {
                    Ok(Some(frame)) => WorkerEvent::Frame { generation, frame },
                    Ok(None) => WorkerEvent::EndOfStream { generation },
                    Err(error) => WorkerEvent::Error {
                        generation,
                        message: error.to_string(),
                    },
                });
            }
            None => {}
        }

        if pending_event.is_none() {
            let limit = prefetch_limit.load(Ordering::SeqCst).max(1);
            if buffered_frame_count.load(Ordering::SeqCst) >= limit {
                thread::sleep(PREFETCH_RETRY_INTERVAL);
                continue;
            }
            pending_event = Some(match video_source.next_frame() {
                Ok(Some(frame)) => WorkerEvent::Frame { generation, frame },
                Ok(None) => WorkerEvent::EndOfStream { generation },
                Err(error) => WorkerEvent::Error {
                    generation,
                    message: error.to_string(),
                },
            });
        }

        let Some(event) = pending_event.take() else {
            continue;
        };
        let frame_generation = frame_event_generation(&event);

        match frame_tx.send(event) {
            Ok(()) => {
                if let Some(generation) = frame_generation
                    && generation == current_generation.load(Ordering::SeqCst)
                {
                    buffered_frame_count.fetch_add(1, Ordering::SeqCst);
                }
            }
            Err(_) => break,
        }
    }
}

fn latest_command(command_rx: &Receiver<WorkerCommand>) -> Option<WorkerCommand> {
    let mut latest = None;

    loop {
        match command_rx.try_recv() {
            Ok(WorkerCommand::Shutdown) => return Some(WorkerCommand::Shutdown),
            Ok(command) => latest = Some(command),
            Err(TryRecvError::Empty) => return latest,
            Err(TryRecvError::Disconnected) => return Some(WorkerCommand::Shutdown),
        }
    }
}

fn frame_event_generation(event: &WorkerEvent) -> Option<u64> {
    match event {
        WorkerEvent::Frame { generation, .. } => Some(*generation),
        _ => None,
    }
}

fn decrement_buffered_frame_count(buffered_frame_count: &AtomicUsize) {
    let _ = buffered_frame_count.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |value| {
        Some(value.saturating_sub(1))
    });
}
