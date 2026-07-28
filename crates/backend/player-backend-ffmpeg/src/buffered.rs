use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender, TryRecvError, TrySendError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use anyhow::{Context, Result};
use player_model::MediaSource;

use crate::{DecodedVideoFrame, FfmpegBackend, MediaProbe, VideoDecodeInfo, VideoFrameSource};

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
    frame_capacity: usize,
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
        let frame_capacity = buffer_capacity.max(1);
        let event_capacity = frame_capacity
            .checked_add(1)
            .context("video predecode buffer capacity is too large")?;
        let (frame_tx, frame_rx) = mpsc::sync_channel(event_capacity);
        let (init_tx, init_rx) = mpsc::channel();
        let current_generation = Arc::new(AtomicU64::new(0));
        let buffered_frame_count = Arc::new(AtomicUsize::new(0));
        let prefetch_limit = Arc::new(AtomicUsize::new(frame_capacity));
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
                frame_capacity,
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
        self.prefetch_limit.store(
            clamp_prefetch_limit(limit, self.frame_capacity),
            Ordering::SeqCst,
        );
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
            } if generation == self.generation => {
                self.ended = true;
                Err(anyhow::anyhow!(message))
            }
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
    frame_tx: SyncSender<WorkerEvent>,
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
    let video_source =
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
    run_buffered_worker(
        video_source,
        command_rx,
        frame_tx,
        current_generation,
        buffered_frame_count,
        prefetch_limit,
    );
}

trait BufferedFrameDecoder {
    fn next_frame(&mut self) -> Result<Option<DecodedVideoFrame>>;

    fn seek_to(&mut self, position: Duration) -> Result<Option<DecodedVideoFrame>>;
}

impl BufferedFrameDecoder for VideoFrameSource {
    fn next_frame(&mut self) -> Result<Option<DecodedVideoFrame>> {
        VideoFrameSource::next_frame(self)
    }

    fn seek_to(&mut self, position: Duration) -> Result<Option<DecodedVideoFrame>> {
        VideoFrameSource::seek_to(self, position)
    }
}

fn run_buffered_worker<D>(
    mut video_source: D,
    command_rx: Receiver<WorkerCommand>,
    frame_tx: SyncSender<WorkerEvent>,
    current_generation: Arc<AtomicU64>,
    buffered_frame_count: Arc<AtomicUsize>,
    prefetch_limit: Arc<AtomicUsize>,
) where
    D: BufferedFrameDecoder,
{
    let mut generation = 0u64;
    let mut pending_event = None;
    let mut terminal_published = false;

    loop {
        let command = if terminal_published {
            Some(wait_for_latest_command(&command_rx))
        } else {
            latest_command(&command_rx)
        };
        match command {
            Some(WorkerCommand::Shutdown) => break,
            Some(WorkerCommand::Seek {
                generation: new_generation,
                position,
            }) => {
                generation = new_generation;
                terminal_published = false;
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
        let is_terminal = is_terminal_event(&event);

        match frame_tx.try_send(event) {
            Ok(()) => {
                if let Some(generation) = frame_generation
                    && generation == current_generation.load(Ordering::SeqCst)
                {
                    buffered_frame_count.fetch_add(1, Ordering::SeqCst);
                }
                terminal_published = is_terminal;
            }
            Err(TrySendError::Full(event)) => {
                pending_event = Some(event);
                thread::sleep(PREFETCH_RETRY_INTERVAL);
            }
            Err(TrySendError::Disconnected(_)) => break,
        }
    }
}

fn wait_for_latest_command(command_rx: &Receiver<WorkerCommand>) -> WorkerCommand {
    let first = match command_rx.recv() {
        Ok(command) => command,
        Err(_) => return WorkerCommand::Shutdown,
    };
    if matches!(first, WorkerCommand::Shutdown) {
        return first;
    }

    latest_command(command_rx).unwrap_or(first)
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

fn is_terminal_event(event: &WorkerEvent) -> bool {
    matches!(
        event,
        WorkerEvent::EndOfStream { .. } | WorkerEvent::Error { .. }
    )
}

fn clamp_prefetch_limit(limit: usize, frame_capacity: usize) -> usize {
    limit.clamp(1, frame_capacity.max(1))
}

fn decrement_buffered_frame_count(buffered_frame_count: &AtomicUsize) {
    let _ = buffered_frame_count.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |value| {
        Some(value.saturating_sub(1))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn test_frame(index: u8) -> DecodedVideoFrame {
        DecodedVideoFrame {
            presentation_time: Duration::from_millis(u64::from(index)),
            width: 1,
            height: 1,
            bytes_per_row: 4,
            pixel_format: crate::VideoPixelFormat::Rgba8888,
            bytes: vec![index, 0, 0, 255],
        }
    }

    #[derive(Debug, Clone, Copy)]
    enum TerminalOutcome {
        EndOfStream,
        Error,
    }

    struct PermanentTerminalDecoder {
        outcome: TerminalOutcome,
        next_calls: Arc<AtomicUsize>,
        seek_calls: Arc<AtomicUsize>,
        release_repeated_call: Arc<AtomicBool>,
    }

    impl BufferedFrameDecoder for PermanentTerminalDecoder {
        fn next_frame(&mut self) -> Result<Option<DecodedVideoFrame>> {
            let call = self.next_calls.fetch_add(1, Ordering::SeqCst) + 1;
            if call > 1 {
                while !self.release_repeated_call.load(Ordering::SeqCst) {
                    thread::sleep(PREFETCH_RETRY_INTERVAL);
                }
            }
            match self.outcome {
                TerminalOutcome::EndOfStream => Ok(None),
                TerminalOutcome::Error => anyhow::bail!("permanent decoder failure"),
            }
        }

        fn seek_to(&mut self, _position: Duration) -> Result<Option<DecodedVideoFrame>> {
            self.seek_calls.fetch_add(1, Ordering::SeqCst);
            match self.outcome {
                TerminalOutcome::EndOfStream => Ok(None),
                TerminalOutcome::Error => anyhow::bail!("permanent decoder failure"),
            }
        }
    }

    #[test]
    fn terminal_event_is_published_once_until_seek() {
        for outcome in [TerminalOutcome::EndOfStream, TerminalOutcome::Error] {
            let (command_tx, command_rx) = mpsc::channel();
            let (frame_tx, frame_rx) = mpsc::sync_channel(2);
            let next_calls = Arc::new(AtomicUsize::new(0));
            let seek_calls = Arc::new(AtomicUsize::new(0));
            let release_repeated_call = Arc::new(AtomicBool::new(false));
            let worker = {
                let next_calls = next_calls.clone();
                let seek_calls = seek_calls.clone();
                let release_repeated_call = release_repeated_call.clone();
                thread::spawn(move || {
                    run_buffered_worker(
                        PermanentTerminalDecoder {
                            outcome,
                            next_calls,
                            seek_calls,
                            release_repeated_call,
                        },
                        command_rx,
                        frame_tx,
                        Arc::new(AtomicU64::new(0)),
                        Arc::new(AtomicUsize::new(0)),
                        Arc::new(AtomicUsize::new(1)),
                    )
                })
            };

            let first = frame_rx
                .recv_timeout(Duration::from_secs(1))
                .expect("first terminal event");
            assert!(matches!(
                (outcome, first),
                (
                    TerminalOutcome::EndOfStream,
                    WorkerEvent::EndOfStream { .. }
                ) | (TerminalOutcome::Error, WorkerEvent::Error { .. })
            ));

            let deadline = Instant::now() + Duration::from_millis(100);
            while next_calls.load(Ordering::SeqCst) < 2 && Instant::now() < deadline {
                thread::sleep(PREFETCH_RETRY_INTERVAL);
            }
            let _ = command_tx.send(WorkerCommand::Shutdown);
            release_repeated_call.store(true, Ordering::SeqCst);
            worker.join().expect("worker shutdown");

            assert_eq!(
                next_calls.load(Ordering::SeqCst),
                1,
                "{outcome:?} must stop decoding after its terminal event"
            );
            assert_eq!(seek_calls.load(Ordering::SeqCst), 0);
        }
    }

    #[test]
    fn seek_rearms_one_terminal_event_for_the_new_generation() {
        for outcome in [TerminalOutcome::EndOfStream, TerminalOutcome::Error] {
            let (command_tx, command_rx) = mpsc::channel();
            let (frame_tx, frame_rx) = mpsc::sync_channel(2);
            let next_calls = Arc::new(AtomicUsize::new(0));
            let seek_calls = Arc::new(AtomicUsize::new(0));
            let release_repeated_call = Arc::new(AtomicBool::new(false));
            let worker = {
                let next_calls = next_calls.clone();
                let seek_calls = seek_calls.clone();
                let release_repeated_call = release_repeated_call.clone();
                thread::spawn(move || {
                    run_buffered_worker(
                        PermanentTerminalDecoder {
                            outcome,
                            next_calls,
                            seek_calls,
                            release_repeated_call,
                        },
                        command_rx,
                        frame_tx,
                        Arc::new(AtomicU64::new(1)),
                        Arc::new(AtomicUsize::new(0)),
                        Arc::new(AtomicUsize::new(1)),
                    )
                })
            };

            let first = frame_rx
                .recv_timeout(Duration::from_secs(1))
                .expect("first terminal event");
            assert_eq!(frame_event_generation(&first), None);
            command_tx
                .send(WorkerCommand::Seek {
                    generation: 1,
                    position: Duration::from_secs(1),
                })
                .expect("seek command");
            let second = frame_rx
                .recv_timeout(Duration::from_secs(1))
                .expect("terminal event after seek");
            assert!(matches!(
                (outcome, second),
                (
                    TerminalOutcome::EndOfStream,
                    WorkerEvent::EndOfStream { generation: 1 }
                ) | (
                    TerminalOutcome::Error,
                    WorkerEvent::Error { generation: 1, .. }
                )
            ));

            thread::sleep(Duration::from_millis(20));
            command_tx
                .send(WorkerCommand::Shutdown)
                .expect("shutdown command");
            worker.join().expect("worker shutdown");

            assert_eq!(next_calls.load(Ordering::SeqCst), 1);
            assert_eq!(seek_calls.load(Ordering::SeqCst), 1);
        }
    }

    struct FiniteFrameDecoder {
        next_index: u8,
        frame_count: u8,
        next_calls: Arc<AtomicUsize>,
    }

    impl BufferedFrameDecoder for FiniteFrameDecoder {
        fn next_frame(&mut self) -> Result<Option<DecodedVideoFrame>> {
            self.next_calls.fetch_add(1, Ordering::SeqCst);
            if self.next_index >= self.frame_count {
                return Ok(None);
            }
            let frame = test_frame(self.next_index);
            self.next_index += 1;
            Ok(Some(frame))
        }

        fn seek_to(&mut self, _position: Duration) -> Result<Option<DecodedVideoFrame>> {
            self.next_index = 0;
            self.next_frame()
        }
    }

    #[test]
    fn terminal_slot_remains_available_after_frame_slots_fill() {
        let (command_tx, command_rx) = mpsc::channel();
        let (frame_tx, frame_rx) = mpsc::sync_channel(3);
        let buffered_frame_count = Arc::new(AtomicUsize::new(0));
        let next_calls = Arc::new(AtomicUsize::new(0));
        let worker = {
            let buffered_frame_count = buffered_frame_count.clone();
            let next_calls = next_calls.clone();
            thread::spawn(move || {
                run_buffered_worker(
                    FiniteFrameDecoder {
                        next_index: 0,
                        frame_count: 2,
                        next_calls,
                    },
                    command_rx,
                    frame_tx,
                    Arc::new(AtomicU64::new(0)),
                    buffered_frame_count,
                    Arc::new(AtomicUsize::new(2)),
                )
            })
        };

        let deadline = Instant::now() + Duration::from_secs(1);
        while buffered_frame_count.load(Ordering::SeqCst) < 2 && Instant::now() < deadline {
            thread::sleep(PREFETCH_RETRY_INTERVAL);
        }
        assert_eq!(buffered_frame_count.load(Ordering::SeqCst), 2);
        assert_eq!(next_calls.load(Ordering::SeqCst), 2);

        let first = frame_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("first frame");
        assert!(matches!(first, WorkerEvent::Frame { .. }));
        decrement_buffered_frame_count(&buffered_frame_count);

        let second = frame_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("second frame");
        assert!(matches!(second, WorkerEvent::Frame { .. }));
        let terminal = frame_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("terminal event in reserved slot");
        assert!(matches!(
            terminal,
            WorkerEvent::EndOfStream { generation: 0 }
        ));
        assert_eq!(next_calls.load(Ordering::SeqCst), 3);

        command_tx
            .send(WorkerCommand::Shutdown)
            .expect("shutdown command");
        worker.join().expect("worker shutdown");
    }

    #[test]
    fn dynamic_prefetch_limit_cannot_exceed_physical_frame_capacity() {
        assert_eq!(clamp_prefetch_limit(0, 4), 1);
        assert_eq!(clamp_prefetch_limit(2, 4), 2);
        assert_eq!(clamp_prefetch_limit(usize::MAX, 4), 4);
    }
}
