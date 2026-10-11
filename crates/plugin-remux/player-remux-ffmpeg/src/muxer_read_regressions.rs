use super::*;
use std::io::{self, Cursor, Read};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct FaultState {
    armed: AtomicBool,
    cancelled: AtomicBool,
    injected: AtomicBool,
    ratios: Mutex<Vec<f32>>,
}

impl ProcessorProgress for FaultState {
    fn on_progress(&self, ratio: f32) {
        self.ratios.lock().unwrap().push(ratio);
        if ratio == 0.05 {
            self.armed.store(true, Ordering::Release);
        }
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

struct FaultReader {
    bytes: Cursor<Vec<u8>>,
    state: Arc<FaultState>,
    cancel: bool,
}

impl Read for FaultReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.state.armed.load(Ordering::Acquire) {
            self.state.injected.store(true, Ordering::Release);
            if self.cancel {
                self.state.cancelled.store(true, Ordering::Release);
                return Err(io::ErrorKind::Interrupted.into());
            }
            return Err(io::Error::from_raw_os_error(ffmpeg::error::EIO));
        }
        self.bytes.read(buffer)
    }
}

fn fault_input(state: Arc<FaultState>, cancel: bool) -> format::context::Input {
    // A valid AAC-LC mono silence frame at 48 kHz, repeated beyond the AVIO
    // window. Probe succeeds; the fault is armed only after the output header.
    const FRAME: [u8; 11] = [255, 241, 76, 64, 1, 127, 252, 1, 24, 32, 7];
    let bytes = FRAME.repeat(20_000);
    let token = state.clone();
    let io = format::context::StreamIo::from_read(FaultReader {
        bytes: Cursor::new(bytes),
        state,
        cancel,
    })
    .expect("create fault IO");
    format::input_from_stream_with_interrupt(io, Some("fixture.aac"), None, move || {
        token.is_cancelled()
    })
    .expect("probe valid AAC before arming read fault")
}

fn run_read_failure(assembly: bool, cancel: bool, preserve_existing: bool) {
    initialize_ffmpeg().expect("initialize FFmpeg");
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "vesper-remux-read-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).unwrap();
    let output = dir.join("output.mp4");
    if preserve_existing {
        fs::write(&output, b"existing target").unwrap();
    }
    let state = Arc::new(FaultState::default());
    let source = AssemblySource {
        path: dir.join("input.aac"),
        kind: StreamKind::Audio,
        content_kind: ContentFormatKind::SingleFile,
    };
    let result = if assembly {
        remux_sources_to_output_with_opener(
            &[source],
            &output,
            OutputFormat::Mp4,
            state.as_ref(),
            |_| Ok(fault_input(state.clone(), cancel)),
        )
    } else {
        remux_input_to_mp4_with_opener(&source.path, &output, state.as_ref(), |_| {
            Ok(fault_input(state.clone(), cancel))
        })
    };
    assert!(
        state.injected.load(Ordering::Acquire),
        "must hit refill fault"
    );
    let size = fs::metadata(&output)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    if result.is_ok() {
        assert!(
            size > 100,
            "baseline must have written actual media packets"
        );
    }
    if cancel {
        assert_eq!(result, Err(ProcessorError::Cancelled));
    } else {
        assert!(
            matches!(&result, Err(ProcessorError::MuxFailed(message)) if message.contains("read") && message.contains("Input/output error")),
            "terminal demux error must remain a failure: {result:?}"
        );
    }
    assert!(!state.ratios.lock().unwrap().contains(&1.0));
    if preserve_existing {
        assert_eq!(fs::read(&output).unwrap(), b"existing target");
    } else {
        assert!(!output.exists(), "failed partial output must be removed");
    }
    assert_eq!(
        fs::read_dir(&dir).unwrap().count(),
        usize::from(preserve_existing),
        "staging files must be removed"
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn single_input_terminal_read_error_is_not_success() {
    run_read_failure(false, false, false);
}

#[test]
fn assembly_terminal_read_error_is_not_success() {
    run_read_failure(true, false, false);
}

#[test]
fn single_input_read_cancel_is_not_success() {
    run_read_failure(false, true, false);
}

#[test]
fn assembly_read_cancel_is_not_success() {
    run_read_failure(true, true, false);
}

#[test]
fn single_read_failure_preserves_existing_output() {
    run_read_failure(false, false, true);
}

#[test]
fn assembly_cancel_preserves_existing_output() {
    run_read_failure(true, true, true);
}

#[test]
fn production_open_failure_preserves_target_and_removes_staging() {
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("output.mp4");
    fs::write(&output, b"existing target").unwrap();
    let state = FaultState::default();
    let result = remux_input_to_mp4(&dir.path().join("missing.aac"), &output, &state);
    assert!(
        matches!(result, Err(ProcessorError::MuxFailed(message)) if message.contains("open input"))
    );
    assert_eq!(fs::read(&output).unwrap(), b"existing target");
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    assert!(!state.ratios.lock().unwrap().contains(&1.0));
}

#[test]
fn clean_eof_publishes_all_packets_and_cleans_staging() {
    initialize_ffmpeg().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.aac");
    let output = dir.path().join("output.mp4");
    const FRAME: [u8; 11] = [255, 241, 76, 64, 1, 127, 252, 1, 24, 32, 7];
    fs::write(&input, FRAME.repeat(100)).unwrap();
    fs::write(&output, b"existing target").unwrap();
    let state = FaultState::default();
    remux_input_to_mp4(&input, &output, &state).unwrap();
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
    assert_eq!(state.ratios.lock().unwrap().last(), Some(&1.0));
    let mut actual = format::input(&output).unwrap();
    let mut packet_count = 0;
    loop {
        let mut packet = ffmpeg::Packet::empty();
        match packet.read(&mut actual) {
            Ok(()) => packet_count += 1,
            Err(ffmpeg::Error::Eof) => break,
            Err(error) => panic!("published output must be complete: {error}"),
        }
    }
    assert_eq!(packet_count, 100);
}

#[test]
fn watcher_latches_one_shot_cancellation_during_actual_avio_refill() {
    struct PulseProgress {
        armed: AtomicBool,
        in_read: AtomicBool,
        pulse: AtomicBool,
    }
    impl ProcessorProgress for PulseProgress {
        fn on_progress(&self, ratio: f32) {
            if ratio == 0.05 {
                self.armed.store(true, Ordering::Release);
            }
        }
        fn is_cancelled(&self) -> bool {
            self.in_read.load(Ordering::Acquire) && self.pulse.swap(false, Ordering::AcqRel)
        }
    }
    struct BlockingReader {
        bytes: Cursor<Vec<u8>>,
        state: Arc<PulseProgress>,
        token: Arc<AtomicBool>,
    }
    impl Read for BlockingReader {
        fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
            if !self.state.armed.load(Ordering::Acquire) {
                return self.bytes.read(bytes);
            }
            self.state.in_read.store(true, Ordering::Release);
            let deadline = std::time::Instant::now() + Duration::from_secs(2);
            while !self.token.load(Ordering::Acquire) {
                if std::time::Instant::now() >= deadline {
                    return Err(io::ErrorKind::TimedOut.into());
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            Err(io::ErrorKind::Interrupted.into())
        }
    }
    initialize_ffmpeg().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let state = Arc::new(PulseProgress {
        armed: AtomicBool::new(false),
        in_read: AtomicBool::new(false),
        pulse: AtomicBool::new(true),
    });
    let output = dir.path().join("output.mp4");
    let result = with_cancellation_token(state.as_ref(), |progress, token| {
        remux_input_to_mp4_with_opener(&dir.path().join("fixture.aac"), &output, progress, |_| {
            const FRAME: [u8; 11] = [255, 241, 76, 64, 1, 127, 252, 1, 24, 32, 7];
            let reader = BlockingReader {
                bytes: Cursor::new(FRAME.repeat(20_000)),
                state: state.clone(),
                token: token.clone(),
            };
            let io = format::context::StreamIo::from_read(reader).unwrap();
            let callback_token = token.clone();
            Ok(
                format::input_from_stream_with_interrupt(
                    io,
                    Some("fixture.aac"),
                    None,
                    move || callback_token.load(Ordering::Acquire),
                )
                .unwrap(),
            )
        })
    });
    assert!(state.in_read.load(Ordering::Acquire));
    assert!(
        !state.pulse.load(Ordering::Acquire),
        "watcher must have accepted the one-shot pulse"
    );
    assert_eq!(result, Err(ProcessorError::Cancelled));
    assert!(!output.exists());
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
}
