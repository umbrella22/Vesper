#![deny(unsafe_code)]

mod aac;
mod auth;
mod hls;
pub mod host;
mod http;
mod mp4;
mod rtp;
mod rtsp;
mod udp;

use std::fmt;

/// A bridge failure without an upstream URL, credentials, or server body.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BridgeError {
    InvalidUrl,
    UnsupportedInput,
    UnsupportedTransport,
    AuthenticationRequired,
    UnsupportedVideo,
    UnsupportedTiming,
    InvalidResponse,
    Unavailable,
    TimedOut,
    MediaLimit,
    Busy,
    LocalHttp,
    Closed,
}

impl BridgeError {
    pub fn code(self) -> &'static str {
        match self {
            Self::InvalidUrl => "rtsp_invalid_url",
            Self::UnsupportedInput => "rtsp_unsupported_input",
            Self::UnsupportedTransport => "rtsp_unsupported_transport",
            Self::AuthenticationRequired => "rtsp_authentication_required",
            Self::UnsupportedVideo => "rtsp_unsupported_video",
            Self::UnsupportedTiming => "rtsp_unsupported_timing",
            Self::InvalidResponse => "rtsp_invalid_response",
            Self::Unavailable => "rtsp_origin_unavailable",
            Self::TimedOut => "rtsp_origin_timeout",
            Self::MediaLimit => "rtsp_bridge_media_limit",
            Self::Busy => "rtsp_bridge_busy",
            Self::LocalHttp => "rtsp_bridge_local_http",
            Self::Closed => "rtsp_bridge_closed",
        }
    }

    pub fn message(self) -> &'static str {
        match self {
            Self::InvalidUrl => "Expected an RTSP URL with a valid host and port.",
            Self::UnsupportedInput => "Expected RTSP/1.0 over unicast.",
            Self::UnsupportedTransport => {
                "The RTSP origin declined the requested unicast transport."
            }
            Self::AuthenticationRequired => {
                "The RTSP origin declined authentication or its challenge is unsupported."
            }
            Self::UnsupportedVideo => "Expected supported H.264 or single-layer 4:2:0 HEVC video.",
            Self::UnsupportedTiming => {
                "Expected strictly increasing video RTP presentation timestamps for low-delay playback."
            }
            Self::InvalidResponse => "The RTSP origin returned an invalid or unsupported response.",
            Self::Unavailable => "The RTSP origin is unavailable or disconnected.",
            Self::TimedOut => "The RTSP origin did not deliver media within the allotted time.",
            Self::MediaLimit => "The stream exceeded the bridge's GOP or buffer limit.",
            Self::Busy => "The maximum number of RTSP bridge sessions is active.",
            Self::LocalHttp => "The RTSP bridge could not start its loopback server.",
            Self::Closed => "The RTSP bridge session is closed.",
        }
    }
}

impl fmt::Display for BridgeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code(), self.message())
    }
}

impl std::error::Error for BridgeError {}

use std::net::{Ipv4Addr, TcpListener};
use std::sync::{
    Arc, Mutex, MutexGuard,
    atomic::{AtomicBool, AtomicUsize, Ordering},
    mpsc::{self, TryRecvError},
};
use std::thread;
use std::time::{Duration, Instant};

use rtsp::Origin;

const MAX_SESSIONS: usize = 2;
const MEDIA_DEADLINE: Duration = Duration::from_secs(5);
const STARTUP_MAX_ATTEMPTS: u32 = 5;
const RECOVERY_TIMEOUT: Duration = Duration::from_secs(20);
const RECOVERY_SEGMENTS: u64 = 2;
static ACTIVE_SESSIONS: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    Connecting,
    Ready,
    Reconnecting,
    Failed(BridgeError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Snapshot {
    pub phase: Phase,
    pub media_sequence: Option<u64>,
    pub audio_omitted: bool,
    pub damaged_access_units: u64,
    pub reconnects: u32,
}

/// Observes when the workers have released a bridge's capacity reservation.
#[derive(Clone)]
pub struct SessionRetirement(Arc<AtomicBool>);

impl SessionRetirement {
    pub fn is_retired(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

struct Reservation {
    retirement: SessionRetirement,
}

impl Reservation {
    fn acquire() -> Result<Self, BridgeError> {
        ACTIVE_SESSIONS
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |active| {
                (active < MAX_SESSIONS).then_some(active + 1)
            })
            .map_err(|_| BridgeError::Busy)?;
        Ok(Self {
            retirement: SessionRetirement(Arc::new(AtomicBool::new(false))),
        })
    }
}

impl Drop for Reservation {
    fn drop(&mut self) {
        ACTIVE_SESSIONS.fetch_sub(1, Ordering::Relaxed);
        self.retirement.0.store(true, Ordering::Release);
    }
}

struct State {
    phase: Phase,
    window: hls::Window,
    audio_omitted: bool,
    damaged_access_units: u64,
    reconnects: u32,
}

struct Shared {
    closed: AtomicBool,
    state: Mutex<State>,
    reservation: Reservation,
}

impl Shared {
    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// Input transport policy. Unicast TCP stays preferred for mobile players.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Transport {
    /// Try UDP unicast when an origin explicitly declines interleaved TCP.
    #[default]
    PreferTcp,
    Tcp,
    Udp,
}

/// An optional RTSP ingest worker and an IPv4 loopback HLS endpoint.
/// Dropping it cancels both workers. A system DNS lookup can finish independently.
#[must_use]
pub struct Bridge {
    shared: Arc<Shared>,
    playlist_url: String,
}

/// A bounded background open for hosts that cannot create sockets on the calling thread.
#[must_use]
pub struct PendingBridge {
    result: mpsc::Receiver<Result<Bridge, BridgeError>>,
    retirement: SessionRetirement,
}

impl PendingBridge {
    /// Takes the result when it becomes available. Dropping an unfinished open closes its bridge.
    pub fn poll(&self) -> Result<Option<Bridge>, BridgeError> {
        match self.result.try_recv() {
            Ok(result) => result.map(Some),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => Err(BridgeError::Closed),
        }
    }
}

impl Bridge {
    /// Reserves capacity before dispatching socket creation to a worker.
    pub fn start_in_background(input: &str) -> Result<PendingBridge, BridgeError> {
        let origin = rtsp::parse_origin(input)?;
        let reservation = Reservation::acquire()?;
        let retirement = reservation.retirement.clone();
        let (sender, receiver) = mpsc::sync_channel(1);
        thread::Builder::new()
            .name("vesper-rtsp-open".into())
            .spawn(move || {
                let _ = sender.send(Self::start_reserved(
                    origin,
                    reservation,
                    Transport::default(),
                ));
            })
            .map_err(|_| BridgeError::LocalHttp)?;
        Ok(PendingBridge {
            result: receiver,
            retirement,
        })
    }

    /// Starts background ingest. The HLS endpoint returns 503 until media is ready.
    pub fn start(input: &str) -> Result<Self, BridgeError> {
        Self::start_with_transport(input, Transport::default())
    }

    pub fn start_with_transport(input: &str, transport: Transport) -> Result<Self, BridgeError> {
        Self::start_reserved(
            rtsp::parse_origin(input)?,
            Reservation::acquire()?,
            transport,
        )
    }

    fn start_reserved(
        origin: Origin,
        reservation: Reservation,
        transport: Transport,
    ) -> Result<Self, BridgeError> {
        let shared = Arc::new(Shared {
            closed: AtomicBool::new(false),
            state: Mutex::new(State {
                phase: Phase::Connecting,
                window: hls::Window::default(),
                audio_omitted: false,
                damaged_access_units: 0,
                reconnects: 0,
            }),
            reservation,
        });
        let listener =
            TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).map_err(|_| BridgeError::LocalHttp)?;
        let address = listener.local_addr().map_err(|_| BridgeError::LocalHttp)?;
        listener
            .set_nonblocking(true)
            .map_err(|_| BridgeError::LocalHttp)?;
        let mut token = [0u8; 16];
        getrandom::fill(&mut token).map_err(|_| BridgeError::LocalHttp)?;
        let token: String = token.iter().map(|byte| format!("{byte:02x}")).collect();
        let bridge = Self {
            playlist_url: format!("http://{address}/{token}/index.m3u8"),
            shared,
        };
        let shared = bridge.shared.clone();
        thread::Builder::new()
            .name("vesper-rtsp-http".into())
            .spawn(move || {
                http::serve(listener, address, token, shared);
            })
            .map_err(|_| BridgeError::LocalHttp)?;
        let shared = bridge.shared.clone();
        thread::Builder::new()
            .name("vesper-rtsp-ingest".into())
            .spawn(move || {
                ingest(&origin, &shared, transport);
            })
            .map_err(|_| BridgeError::LocalHttp)?;
        Ok(bridge)
    }

    /// A capability URL for the native player. Keep it inside the playback session.
    pub fn playlist_url(&self) -> &str {
        &self.playlist_url
    }

    pub fn snapshot(&self) -> Snapshot {
        let state = self.shared.state();
        Snapshot {
            phase: state.phase,
            media_sequence: state.window.media_sequence(),
            audio_omitted: state.audio_omitted,
            damaged_access_units: state.damaged_access_units,
            reconnects: state.reconnects,
        }
    }
}

impl Drop for Bridge {
    fn drop(&mut self) {
        self.shared.closed.store(true, Ordering::Relaxed);
    }
}

#[derive(Default)]
struct RetrySchedule {
    attempts: u32,
    recovery_deadline: Option<Instant>,
}

impl RetrySchedule {
    fn after_failure(
        &mut self,
        now: Instant,
        was_ready: bool,
        error: BridgeError,
    ) -> Option<Instant> {
        if !matches!(error, BridgeError::Unavailable | BridgeError::TimedOut) {
            return None;
        }
        if was_ready {
            self.attempts = 0;
            self.recovery_deadline = Some(now + RECOVERY_TIMEOUT);
        }
        self.attempts += 1;
        let retry_at = now + Duration::from_millis(500) * self.attempts.min(4);
        match self.recovery_deadline {
            Some(deadline) if now < deadline => Some(retry_at.min(deadline)),
            None if self.attempts < STARTUP_MAX_ATTEMPTS => Some(retry_at),
            _ => None,
        }
    }
}

fn ingest(origin: &Origin, shared: &Shared, transport: Transport) {
    let mut retry = RetrySchedule::default();
    while !shared.closed.load(Ordering::Relaxed) {
        let Err(error) = ingest_connection(origin, shared, retry.recovery_deadline, transport)
        else {
            break;
        };
        if shared.closed.load(Ordering::Relaxed) {
            break;
        }
        let retry_at = {
            let mut state = shared.state();
            let Some(retry_at) =
                retry.after_failure(Instant::now(), state.phase == Phase::Ready, error)
            else {
                state.phase = Phase::Failed(error);
                break;
            };
            state.phase = Phase::Reconnecting;
            state.reconnects = state.reconnects.saturating_add(1);
            retry_at
        };
        while !shared.closed.load(Ordering::Relaxed) {
            let remaining = retry_at.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            thread::sleep(remaining.min(Duration::from_millis(100)));
        }
    }
}

fn ingest_connection(
    origin: &Origin,
    shared: &Shared,
    mut recovery_deadline: Option<Instant>,
    transport: Transport,
) -> Result<(), BridgeError> {
    let (mut session, tracks) =
        rtsp::Session::open(origin, &shared.closed, recovery_deadline, transport)?;
    shared.state().audio_omitted |= tracks.audio_omitted;
    let mut depacketizer = rtp::VideoAssembler::new(tracks.video.config);
    let mut packager = hls::Segmenter::default();
    let play_start = Instant::now();
    let mut audio = tracks.audio.map(|track| {
        (
            track.payload_type,
            aac::Timeline::new(track.config, tracks.start_clocks),
        )
    });
    let first_sequence = shared.state().window.next_sequence;
    let mut sequence = first_sequence;
    let mut media_deadline = Instant::now() + MEDIA_DEADLINE;
    loop {
        let deadline = recovery_deadline.map_or(media_deadline, |limit| limit.min(media_deadline));
        if Instant::now() >= deadline {
            return Err(BridgeError::TimedOut);
        }
        let Some(media) = session.next_rtp(deadline)? else {
            continue;
        };
        if media.track == 1 {
            if let Some((payload, timeline)) = &mut audio
                && let Some(packet) = rtp::Packet::parse(&media.bytes)
                && packet.payload_type == *payload
            {
                timeline.push(&packet, play_start.elapsed());
            }
            continue;
        }
        let (damaged_before, config_before) =
            (depacketizer.damaged_units, depacketizer.config_changes);
        let unit = match rtp::Packet::parse(&media.bytes) {
            Some(packet) if packet.payload_type == tracks.video.payload_type => {
                depacketizer.push(&packet)
            }
            Some(_) => continue,
            None => {
                depacketizer.reject_packet();
                None
            }
        };
        if depacketizer.unsupported_timing {
            return Err(BridgeError::UnsupportedTiming);
        }
        if damaged_before != depacketizer.damaged_units {
            let mut state = shared.state();
            state.damaged_access_units = state
                .damaged_access_units
                .saturating_add(depacketizer.damaged_units - damaged_before);
            packager.reset();
        } else if config_before != depacketizer.config_changes {
            packager.reset();
        }
        let Some(unit) = unit else {
            continue;
        };
        let audio_time = if let Some((_, timeline)) = &mut audio {
            let ticks = timeline
                .video_ticks(unit.timestamp, play_start.elapsed())
                .ok_or(BridgeError::UnsupportedTiming)?;
            Some((timeline, ticks))
        } else {
            None
        };
        if let Some(segment) = packager.push(unit, &depacketizer.config, sequence, audio_time)? {
            let now = Instant::now();
            if now >= deadline {
                return Err(BridgeError::TimedOut);
            }
            let mut state = shared.state();
            state.window.publish(segment, now)?;
            sequence = state.window.next_sequence;
            // A lone segment from a flapping origin cannot renew the recovery budget.
            if state.window.ready()
                && (recovery_deadline.is_none() || sequence - first_sequence >= RECOVERY_SEGMENTS)
            {
                state.phase = Phase::Ready;
                recovery_deadline = None;
            }
            media_deadline = now + MEDIA_DEADLINE;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retirement_waits_for_the_workers_and_leaves_the_other_reservation_held() {
        let first = Arc::new(Reservation::acquire().expect("first slot"));
        let second = Reservation::acquire().expect("second slot");
        let worker = Arc::clone(&first);
        let first_retirement = first.retirement.clone();
        let second_retirement = second.retirement.clone();
        drop(first);
        assert!(!first_retirement.is_retired());
        assert!(matches!(Reservation::acquire(), Err(BridgeError::Busy)));
        drop(worker);
        assert!(first_retirement.is_retired());
        assert!(!second_retirement.is_retired());
        let _replacement = Reservation::acquire().expect("released first slot");
    }

    #[test]
    fn ready_origin_gets_more_than_the_startup_retry_budget() {
        let start = Instant::now();
        let mut retry = RetrySchedule::default();
        let mut now = start;
        for attempt in 0..10 {
            now = retry
                .after_failure(now, attempt == 0, BridgeError::Unavailable)
                .expect("transient error within the recovery window");
        }
        assert!(now < start + RECOVERY_TIMEOUT);
        assert_eq!(retry.recovery_deadline, Some(start + RECOVERY_TIMEOUT));
        assert_eq!(
            retry.after_failure(start + RECOVERY_TIMEOUT, false, BridgeError::TimedOut),
            None
        );
        assert_eq!(
            retry.after_failure(now, false, BridgeError::AuthenticationRequired),
            None
        );
    }

    #[test]
    fn startup_still_exhausts_its_attempt_budget() {
        let mut retry = RetrySchedule::default();
        let mut now = Instant::now();
        for _ in 1..STARTUP_MAX_ATTEMPTS {
            now = retry
                .after_failure(now, false, BridgeError::Unavailable)
                .expect("startup retry");
        }
        assert_eq!(
            retry.after_failure(now, false, BridgeError::Unavailable),
            None
        );
    }
}
