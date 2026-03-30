use std::sync::mpsc::{self, Receiver, Sender, SyncSender, TryRecvError, TrySendError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use anyhow::{Context, Result};
use player_core::MediaSource;

use crate::{DecodedVideoFrame, FfmpegBackend, VideoDecodeInfo};

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
    ended: bool,
    worker: Option<JoinHandle<()>>,
}

#[derive(Debug)]
pub struct BufferedVideoSourceBootstrap {
    pub source: BufferedVideoSource,
    pub decode_info: VideoDecodeInfo,
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

impl BufferedVideoSource {
    pub fn new(
        source: MediaSource,
        buffer_capacity: usize,
    ) -> Result<BufferedVideoSourceBootstrap> {
        let (command_tx, command_rx) = mpsc::channel();
        let (frame_tx, frame_rx) = mpsc::sync_channel(buffer_capacity.max(1));
        let (init_tx, init_rx) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("ffmpeg-video-prefetch".to_owned())
            .spawn(move || worker_loop(source, command_rx, frame_tx, init_tx))
            .context("failed to spawn video predecode worker")?;
        let decode_info = init_rx
            .recv()
            .context("video predecode worker disconnected before reporting decoder info")??;

        Ok(BufferedVideoSourceBootstrap {
            source: Self {
                command_tx,
                frame_rx,
                generation: 0,
                ended: false,
                worker: Some(worker),
            },
            decode_info,
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
        self.ended = false;
        self.command_tx
            .send(WorkerCommand::Seek {
                generation: self.generation,
                position,
            })
            .context("failed to send seek request to video predecode worker")?;

        self.recv_frame()
    }

    fn handle_event(&mut self, event: WorkerEvent) -> Result<Option<DecodedVideoFrame>> {
        match event {
            WorkerEvent::Frame { generation, frame } if generation == self.generation => {
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
    command_rx: Receiver<WorkerCommand>,
    frame_tx: SyncSender<WorkerEvent>,
    init_tx: Sender<Result<VideoDecodeInfo>>,
) {
    let backend = match FfmpegBackend::new() {
        Ok(backend) => backend,
        Err(error) => {
            let _ = init_tx.send(Err(anyhow::anyhow!(error.to_string())));
            let _ = frame_tx.try_send(WorkerEvent::Error {
                generation: 0,
                message: error.to_string(),
            });
            return;
        }
    };
    let mut source = match backend.open_video_source(source) {
        Ok(source) => source,
        Err(error) => {
            let _ = init_tx.send(Err(anyhow::anyhow!(error.to_string())));
            let _ = frame_tx.try_send(WorkerEvent::Error {
                generation: 0,
                message: error.to_string(),
            });
            return;
        }
    };
    let _ = init_tx.send(Ok(source.decode_info().clone()));
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
                pending_event = Some(match source.seek_to(position) {
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
            pending_event = Some(match source.next_frame() {
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

        match frame_tx.try_send(event) {
            Ok(()) => {}
            Err(TrySendError::Full(event)) => {
                pending_event = Some(event);
                thread::sleep(PREFETCH_RETRY_INTERVAL);
            }
            Err(TrySendError::Disconnected(_)) => break,
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
