use std::io::{self, BufReader, Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use base64::{Engine, engine::general_purpose::STANDARD};
use url::Url;

use crate::{
    BridgeError as Error, Transport, aac,
    auth::{Challenge, Credentials},
    rtp::{MAX_PARAMETER_SET_BYTES, VideoCodec, VideoConfig},
    udp,
};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(3);
const SOCKET_POLL: Duration = Duration::from_millis(200);
const MAX_URL_BYTES: usize = 4096;
const MAX_HEADERS: usize = 8192;
const MAX_BODY: usize = 64 * 1024;
const MAX_MEDIA_SECTIONS: usize = 16;

pub(crate) struct Origin {
    uri: Url,
    credentials: Option<Credentials>,
}

pub(crate) fn parse_origin(input: &str) -> Result<Origin, Error> {
    let mut uri = parse_uri(input)?;
    let credentials = Credentials::take(&mut uri)?;
    Ok(Origin { uri, credentials })
}

fn parse_uri(input: &str) -> Result<Url, Error> {
    if input.len() > MAX_URL_BYTES || input.bytes().any(|byte| byte.is_ascii_control()) {
        return Err(Error::InvalidUrl);
    }
    let url = Url::parse(input).map_err(|_| Error::InvalidUrl)?;
    if url.host_str().is_none() || url.port() == Some(0) {
        return Err(Error::InvalidUrl);
    }
    if url.scheme() != "rtsp"
        || url.fragment().is_some()
        || host(&url)?
            .parse::<IpAddr>()
            .is_ok_and(|ip| ip.is_multicast())
    {
        return Err(Error::UnsupportedInput);
    }
    Ok(url)
}

fn host(url: &Url) -> Result<&str, Error> {
    url.host_str()
        .map(|s| s.trim_start_matches('[').trim_end_matches(']'))
        .ok_or(Error::InvalidUrl)
}

fn resolve_control(origin: &Url, base: &Url, value: &str) -> Result<Url, Error> {
    let resolved = base.join(value).map_err(|_| Error::InvalidResponse)?;
    let validated = parse_uri(resolved.as_str())?;
    if !validated.username().is_empty()
        || validated.password().is_some()
        || validated.host_str() != origin.host_str()
        || validated.port().unwrap_or(554) != origin.port().unwrap_or(554)
    {
        return Err(Error::UnsupportedInput);
    }
    Ok(validated)
}

struct ControlUris<'a> {
    origin: &'a Url,
    presentation: Url,
    media_base: Url,
    implicit_base: bool,
}

impl<'a> ControlUris<'a> {
    fn from_description(origin: &'a Url, description: &Message) -> Result<Self, Error> {
        let header = match description.field("Content-Base")? {
            Some(base) => Some(base),
            None => description.field("Content-Location")?,
        };
        let presentation = match header {
            Some(base) => resolve_control(origin, origin, base)?,
            None => origin.clone(),
        };
        let mut media_base = presentation.clone();
        // Keep support for servers that omit both headers and use track IDs
        // beneath the presentation path. Only media controls use this base.
        if header.is_none() && !media_base.path().ends_with('/') {
            let directory = format!("{}/", media_base.path());
            media_base.set_path(&directory);
        }
        Ok(Self {
            origin,
            presentation,
            media_base,
            implicit_base: header.is_none(),
        })
    }

    fn aggregate(&self, control: Option<&str>) -> Result<Url, Error> {
        match control {
            Some(value) if value != "*" => resolve_control(self.origin, &self.presentation, value),
            _ => Ok(self.presentation.clone()),
        }
    }

    fn track(&self, value: &str) -> Result<Url, Error> {
        let mut target = resolve_control(self.origin, &self.media_base, value)?;
        if self.implicit_base
            && target.query().is_none()
            && self.origin.query().is_some()
            && target.path().starts_with(self.media_base.path())
            && !value.starts_with(['/', '?', '#'])
            && !matches!(value.split('/').next(), Some("." | ".."))
            && matches!(
                Url::parse(value),
                Err(url::ParseError::RelativeUrlWithoutBase)
            )
        {
            // Some origins keep camera/channel selectors on relative track IDs.
            // Explicit track queries and absolute targets own their own selectors.
            target.set_query(self.origin.query());
            if target.as_str().len() > MAX_URL_BYTES {
                return Err(Error::InvalidUrl);
            }
        }
        Ok(target)
    }
}

fn check_open(closed: &AtomicBool) -> Result<(), Error> {
    if closed.load(Ordering::Relaxed) {
        return Err(Error::Closed);
    }
    Ok(())
}

fn request_deadline(limit: Option<Instant>) -> Instant {
    let deadline = Instant::now() + REQUEST_TIMEOUT;
    limit.map_or(deadline, |limit| limit.min(deadline))
}

fn check_deadline(deadline: Instant) -> Result<(), Error> {
    if Instant::now() >= deadline {
        return Err(Error::TimedOut);
    }
    Ok(())
}

fn connect(origin: &Url, closed: &AtomicBool, limit: Option<Instant>) -> Result<TcpStream, Error> {
    let port = origin.port().unwrap_or(554);
    let host = host(origin)?;
    let deadline = request_deadline(limit);
    check_open(closed)?;
    check_deadline(deadline)?;
    // The system resolver runs only on the bounded ingest worker pool. A dropped
    // bridge can shut down its loopback server without joining a resolver call.
    let addresses = (host, port)
        .to_socket_addrs()
        .map_err(|_| Error::Unavailable)?;
    let mut unicast = false;
    for address in addresses.take(8).filter(|addr| !addr.ip().is_multicast()) {
        unicast = true;
        check_open(closed)?;
        check_deadline(deadline)?;
        if let Ok(socket) = dial(address, deadline) {
            socket
                .set_read_timeout(Some(SOCKET_POLL))
                .map_err(|_| Error::Unavailable)?;
            socket
                .set_write_timeout(Some(SOCKET_POLL))
                .map_err(|_| Error::Unavailable)?;
            return Ok(socket);
        }
    }
    if unicast {
        Err(Error::Unavailable)
    } else {
        Err(Error::UnsupportedInput)
    }
}

fn dial(addr: SocketAddr, deadline: Instant) -> io::Result<TcpStream> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    TcpStream::connect_timeout(&addr, remaining.min(Duration::from_secs(1)))
}

struct Message {
    header: String,
    body: Vec<u8>,
}

impl Message {
    fn field(&self, name: &str) -> Result<Option<&str>, Error> {
        header_field(&self.header, name)
    }

    fn required(&self, name: &str) -> Result<&str, Error> {
        self.field(name)?.ok_or(Error::InvalidResponse)
    }

    fn cseq(&self) -> Result<u32, Error> {
        self.required("CSeq")?
            .parse()
            .map_err(|_| Error::InvalidResponse)
    }

    fn status(&self) -> Result<Option<u16>, Error> {
        let mut words = self
            .header
            .lines()
            .next()
            .ok_or(Error::InvalidResponse)?
            .split_ascii_whitespace();
        match words.next() {
            Some("RTSP/1.0") => Ok(Some(
                words
                    .next()
                    .ok_or(Error::InvalidResponse)?
                    .parse()
                    .map_err(|_| Error::InvalidResponse)?,
            )),
            Some(method) if method.bytes().all(|b| b.is_ascii_uppercase() || b == b'_') => Ok(None),
            _ => Err(Error::InvalidResponse),
        }
    }
}

fn header_field<'a>(header: &'a str, name: &str) -> Result<Option<&'a str>, Error> {
    let mut values = header.lines().skip(1).filter_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.eq_ignore_ascii_case(name).then(|| value.trim())
    });
    let value = values.next();
    if values.next().is_some() {
        return Err(Error::InvalidResponse);
    }
    Ok(value)
}

enum Frame {
    Message(Message),
    Interleaved(u8, Vec<u8>),
}

struct Wire<'a> {
    stream: BufReader<TcpStream>,
    closed: &'a AtomicBool,
    cseq: u32,
    credentials: Option<&'a Credentials>,
    challenge: Option<Challenge>,
}

impl Wire<'_> {
    fn read(&mut self, bytes: &mut [u8], deadline: Instant) -> Result<Option<usize>, Error> {
        check_open(self.closed)?;
        if Instant::now() >= deadline {
            return Ok(None);
        }
        match self.stream.read(bytes) {
            Ok(0) => Err(Error::Unavailable),
            Ok(count) => Ok(Some(count)),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::Interrupted
                        | io::ErrorKind::TimedOut
                        | io::ErrorKind::WouldBlock
                ) =>
            {
                Ok(None)
            }
            Err(_) => Err(Error::Unavailable),
        }
    }

    fn read_exact(&mut self, mut bytes: &mut [u8], deadline: Instant) -> Result<(), Error> {
        while !bytes.is_empty() {
            if Instant::now() >= deadline {
                return Err(Error::TimedOut);
            }
            if let Some(count) = self.read(bytes, deadline)? {
                bytes = &mut bytes[count..];
            }
        }
        Ok(())
    }

    fn next(&mut self, deadline: Instant) -> Result<Option<Frame>, Error> {
        let mut first = [0];
        if self.read(&mut first, deadline)?.is_none() {
            return Ok(None);
        }
        if first[0] == b'$' {
            let mut header = [0; 3];
            self.read_exact(&mut header, deadline)?;
            let size = usize::from(u16::from_be_bytes([header[1], header[2]]));
            let mut payload = vec![0; size];
            self.read_exact(&mut payload, deadline)?;
            return Ok(Some(Frame::Interleaved(header[0], payload)));
        }
        let mut header = Vec::from(first);
        // Read only up to the header boundary, even when the next frame is binary.
        while !header.ends_with(b"\r\n\r\n") && !header.ends_with(b"\n\n") {
            if header.len() >= MAX_HEADERS {
                return Err(Error::InvalidResponse);
            }
            let mut byte = [0];
            self.read_exact(&mut byte, deadline)?;
            header.push(byte[0]);
        }
        let header = String::from_utf8(header).map_err(|_| Error::InvalidResponse)?;
        let size = header_field(&header, "Content-Length")?
            .map(str::parse::<usize>)
            .transpose()
            .map_err(|_| Error::InvalidResponse)?
            .unwrap_or(0);
        if size > MAX_BODY {
            return Err(Error::InvalidResponse);
        }
        let mut body = vec![0; size];
        self.read_exact(&mut body, deadline)?;
        Ok(Some(Frame::Message(Message { header, body })))
    }

    fn send(&mut self, request: &str) -> Result<(), Error> {
        self.stream
            .get_mut()
            .write_all(request.as_bytes())
            .map_err(|_| Error::Unavailable)
    }

    fn request(&mut self, method: &str, uri: &Url, fields: &str) -> Result<u32, Error> {
        self.cseq = self.cseq.checked_add(1).ok_or(Error::InvalidResponse)?;
        let authorization = match (self.challenge.as_mut(), self.credentials) {
            (Some(challenge), Some(credentials)) => format!(
                "Authorization: {}\r\n",
                challenge.authorization(credentials, method, uri)?
            ),
            _ => String::new(),
        };
        self.send(&format!(
            "{method} {uri} RTSP/1.0\r\nCSeq: {}\r\nUser-Agent: Vesper\r\n{authorization}{fields}\r\n",
            self.cseq
        ))?;
        Ok(self.cseq)
    }

    fn reply_to_server(&mut self, request: &Message) -> Result<(), Error> {
        let status = if request.header.starts_with("GET_PARAMETER ")
            || request.header.starts_with("OPTIONS ")
        {
            "200 OK"
        } else {
            "501 Not Implemented"
        };
        self.send(&format!(
            "RTSP/1.0 {status}\r\nCSeq: {}\r\nContent-Length: 0\r\n\r\n",
            request.cseq()?
        ))
    }

    fn update_auth_challenge(&mut self, reply: &Message) -> Result<(), Error> {
        if self.credentials.is_none() {
            return Err(Error::AuthenticationRequired);
        }
        let challenge = Challenge::select(reply.header.lines().skip(1).filter_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.eq_ignore_ascii_case("WWW-Authenticate")
                .then(|| value.trim())
        }))
        .ok_or(Error::AuthenticationRequired)?;
        if self.challenge.is_some() && !challenge.stale() {
            return Err(Error::AuthenticationRequired);
        }
        self.challenge = Some(challenge);
        Ok(())
    }

    fn exchange(
        &mut self,
        method: &str,
        uri: &Url,
        fields: &str,
        limit: Option<Instant>,
    ) -> Result<Message, Error> {
        let deadline = request_deadline(limit);
        check_open(self.closed)?;
        check_deadline(deadline)?;
        let mut cseq = self.request(method, uri, fields)?;
        let mut retries = 0;
        while Instant::now() < deadline {
            if let Some(Frame::Message(message)) = self.next(deadline)? {
                if let Some(status) = message.status()? {
                    if message.cseq()? == cseq {
                        if status == 401 && retries < 2 {
                            self.update_auth_challenge(&message)?;
                            cseq = self.request(method, uri, fields)?;
                            retries += 1;
                            continue;
                        }
                        response_status(status)?;
                        return Ok(message);
                    }
                } else {
                    self.reply_to_server(&message)?;
                }
            }
        }
        Err(Error::TimedOut)
    }
}

fn response_status(status: u16) -> Result<(), Error> {
    match status {
        200 => Ok(()),
        401 | 403 => Err(Error::AuthenticationRequired),
        301 | 302 => Err(Error::UnsupportedInput),
        461 => Err(Error::UnsupportedTransport),
        404 | 408 | 454 | 500 | 503 | 504 => Err(Error::Unavailable),
        _ => Err(Error::InvalidResponse),
    }
}

pub(crate) struct Track {
    pub payload_type: u8,
    pub config: VideoConfig,

    control: Url,
}

pub(crate) struct AudioTrack {
    pub payload_type: u8,
    pub config: aac::Config,
    control: Url,
}

pub(crate) struct Tracks {
    pub video: Track,
    pub audio: Option<AudioTrack>,
    pub audio_omitted: bool,
    // RTP timestamps corresponding to the same PLAY position, when supplied.
    pub start_clocks: Option<(u32, u32)>,
}

fn payload_attr<'a>(section: &'a [&str], prefix: &str, pt: u8) -> Option<&'a str> {
    section.iter().find_map(|line| {
        let (value, attributes) = line.strip_prefix(prefix)?.split_once(char::is_whitespace)?;
        (value.parse::<u8>().ok() == Some(pt)).then(|| attributes.trim())
    })
}

fn decode_parameter_set(value: &str) -> Result<Vec<u8>, Error> {
    if value.len() > MAX_PARAMETER_SET_BYTES.div_ceil(3) * 4 {
        return Err(Error::UnsupportedVideo);
    }
    let bytes = STANDARD.decode(value).map_err(|_| Error::InvalidResponse)?;
    if bytes.is_empty() || bytes.len() > MAX_PARAMETER_SET_BYTES {
        return Err(Error::UnsupportedVideo);
    }
    Ok(bytes)
}

fn parse_parameter_sets(
    config: &mut VideoConfig,
    value: &str,
    required_kind: Option<u8>,
) -> Result<(), Error> {
    for encoded in value.split(',') {
        let nal = decode_parameter_set(encoded.trim())?;
        let kind = config
            .parameter_set_type(&nal)
            .ok_or(Error::UnsupportedVideo)?;
        if required_kind.is_some_and(|required| required != kind) {
            return Err(Error::UnsupportedVideo);
        }
        config
            .set_parameter_set(kind, &nal)
            .ok_or(Error::UnsupportedVideo)?;
    }
    Ok(())
}

fn parse_track(uris: &ControlUris<'_>, description: &str) -> Result<(Url, Tracks), Error> {
    let lines: Vec<_> = description.lines().map(str::trim).collect();
    let mut starts = lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| line.starts_with("m=").then_some(index))
        .peekable();
    let sections = std::iter::from_fn(|| {
        let start = starts.next()?;
        Some(&lines[start..starts.peek().copied().unwrap_or(lines.len())])
    });
    let session = lines.iter().take_while(|line| !line.starts_with("m="));
    let aggregate = uris.aggregate(
        session
            .filter_map(|line| line.strip_prefix("a=control:"))
            .next(),
    )?;
    let mut track = None;
    let mut audio = None;
    let mut audio_advertised = false;
    for (index, section) in sections.enumerate() {
        if index >= MAX_MEDIA_SECTIONS {
            return Err(Error::InvalidResponse);
        }
        let mut media = section[0].split_ascii_whitespace();
        let kind = media.next();
        audio_advertised |= kind == Some("m=audio");
        if !matches!(kind, Some("m=video" | "m=audio")) {
            continue;
        }
        let _port = media.next();
        if media.next() != Some("RTP/AVP") {
            continue;
        }
        let control = || {
            let value = section
                .iter()
                .find_map(|line| line.strip_prefix("a=control:"))
                .filter(|control| !control.is_empty() && *control != "*")
                .ok_or(Error::InvalidResponse)?;
            uris.track(value)
        };
        if kind == Some("m=audio") {
            if audio.is_none() {
                for pt in media
                    .filter_map(|value| value.parse::<u8>().ok())
                    .filter(|pt| *pt <= 127)
                {
                    if let Some(config) =
                        payload_attr(section, "a=rtpmap:", pt).and_then(|encoding| {
                            aac::Config::parse(encoding, payload_attr(section, "a=fmtp:", pt)?)
                        })
                    {
                        audio = Some(AudioTrack {
                            payload_type: pt,
                            config,
                            control: control()?,
                        });
                        break;
                    }
                }
            }
            continue;
        }
        if track.is_some() {
            continue;
        }
        for pt in media
            .filter_map(|value| value.parse::<u8>().ok())
            .filter(|pt| *pt <= 127)
        {
            let Some(encoding) = payload_attr(section, "a=rtpmap:", pt) else {
                continue;
            };
            let codec = if encoding.eq_ignore_ascii_case("H264/90000") {
                VideoCodec::Avc
            } else if encoding.eq_ignore_ascii_case("H265/90000") {
                VideoCodec::Hevc
            } else {
                continue;
            };
            let mut config = VideoConfig {
                codec,
                ..VideoConfig::default()
            };
            let mut mode_supported = true;
            for (key, value) in payload_attr(section, "a=fmtp:", pt)
                .unwrap_or("")
                .split(';')
                .filter_map(|entry| entry.trim().split_once('='))
            {
                let key = key.trim().to_ascii_lowercase();
                let value = value.trim();
                match (codec, key.as_str()) {
                    (VideoCodec::Avc, "packetization-mode") => {
                        mode_supported &= matches!(value, "0" | "1")
                    }
                    (VideoCodec::Avc, "sprop-parameter-sets") => {
                        parse_parameter_sets(&mut config, value, None)?;
                    }
                    (VideoCodec::Hevc, "sprop-max-don-diff") => mode_supported &= value == "0",
                    (VideoCodec::Hevc, "sprop-vps") => {
                        parse_parameter_sets(&mut config, value, Some(32))?;
                    }
                    (VideoCodec::Hevc, "sprop-sps") => {
                        parse_parameter_sets(&mut config, value, Some(33))?;
                    }
                    (VideoCodec::Hevc, "sprop-pps") => {
                        parse_parameter_sets(&mut config, value, Some(34))?;
                    }
                    _ => (),
                }
            }
            if !mode_supported {
                continue;
            }
            track = Some(Track {
                payload_type: pt,
                config,
                control: control()?,
            });
            break;
        }
    }
    let video = track.ok_or(Error::UnsupportedVideo)?;
    let audio_omitted = audio_advertised && audio.is_none();
    Ok((
        aggregate,
        Tracks {
            video,
            audio,
            audio_omitted,
            start_clocks: None,
        },
    ))
}

fn session_fields(response: &Message) -> Result<(String, Duration), Error> {
    let mut fields = response.required("Session")?.split(';').map(str::trim);
    let value = fields
        .next()
        .filter(|value| {
            !value.is_empty()
                && value.len() <= 128
                && value.bytes().all(|byte| byte.is_ascii_graphic())
        })
        .ok_or(Error::InvalidResponse)?;
    let timeout = fields
        .find_map(|field| field.strip_prefix("timeout=")?.parse::<u64>().ok())
        .unwrap_or(60);
    Ok((
        format!("Session: {value}\r\n"),
        Duration::from_secs(timeout.clamp(4, 120) / 2),
    ))
}

fn rtp_channel(response: &Message) -> Result<u8, Error> {
    let mut fields = response.required("Transport")?.split(';').map(str::trim);
    if !fields
        .next()
        .is_some_and(|value| value.eq_ignore_ascii_case("RTP/AVP/TCP"))
    {
        return Err(Error::UnsupportedInput);
    }
    let mut channel = None;
    for field in fields {
        if field.eq_ignore_ascii_case("multicast") {
            return Err(Error::UnsupportedInput);
        }
        if let Some((key, value)) = field.split_once('=')
            && key.eq_ignore_ascii_case("interleaved")
        {
            let (rtp, rtcp) = value.split_once('-').ok_or(Error::InvalidResponse)?;
            let (rtp, rtcp) = (rtp.parse::<u8>(), rtcp.parse::<u8>());
            match (rtp, rtcp) {
                (Ok(rtp), Ok(rtcp)) if Some(rtcp) == rtp.checked_add(1) && channel.is_none() => {
                    channel = Some(rtp)
                }
                _ => return Err(Error::InvalidResponse),
            }
        }
    }
    channel.ok_or(Error::UnsupportedInput)
}

enum MediaTransport {
    Interleaved(u8),
    Datagram(udp::Pair),
}

fn setup(
    wire: &mut Wire<'_>,
    control: &Url,
    session: &str,
    channel: u8,
    datagram: bool,
    deadline: Option<Instant>,
) -> Result<(MediaTransport, String, Duration), Error> {
    let pair = if datagram {
        Some(udp::Pair::bind(wire.stream.get_ref())?)
    } else {
        None
    };
    let requested = match &pair {
        Some(pair) => format!(
            "RTP/AVP;unicast;client_port={}-{}",
            pair.client_port,
            pair.client_port + 1
        ),
        None => format!("RTP/AVP/TCP;unicast;interleaved={channel}-{}", channel + 1),
    };
    let reply = wire.exchange(
        "SETUP",
        control,
        &format!("{session}Transport: {requested}\r\n"),
        deadline,
    )?;
    let (new_session, timeout) = session_fields(&reply)?;
    if !session.is_empty() && session != new_session {
        return Err(Error::InvalidResponse);
    }
    let transport = if let Some(pair) = pair {
        pair.connect(wire.stream.get_ref(), reply.required("Transport")?)?;
        MediaTransport::Datagram(pair)
    } else {
        MediaTransport::Interleaved(rtp_channel(&reply)?)
    };
    Ok((transport, new_session, timeout))
}

fn rtp_clock(header: &str, control: &Url, uris: &ControlUris<'_>) -> Option<u32> {
    for entry in header.split(',') {
        let mut uri = None;
        let mut clock = None;
        for (key, value) in entry
            .split(';')
            .filter_map(|field| field.trim().split_once('='))
        {
            if key.eq_ignore_ascii_case("url") {
                uri = uris.track(value).ok();
            }
            if key.eq_ignore_ascii_case("rtptime") {
                clock = value.parse().ok();
            }
        }
        if uri.as_ref() == Some(control) {
            return clock;
        }
    }
    None
}

pub(crate) struct MediaPacket {
    pub track: usize,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Copy)]
struct PendingKeepalive {
    cseq: u32,
    deadline: Instant,
    retried: bool,
}

pub(crate) struct Session<'a> {
    wire: Wire<'a>,
    aggregate: Url,
    session: String,
    media: [Option<MediaTransport>; 2],
    datagram: bool,
    next_udp_track: usize,
    next_control_poll: Instant,
    keepalive: Duration,
    next_keepalive: Instant,
    pending: Option<PendingKeepalive>,
}

impl<'a> Session<'a> {
    pub fn open(
        origin: &'a Origin,
        closed: &'a AtomicBool,
        limit: Option<Instant>,
        transport: Transport,
    ) -> Result<(Self, Tracks), Error> {
        let mut wire = Wire {
            stream: BufReader::new(connect(&origin.uri, closed, limit)?),
            closed,
            cseq: 0,
            credentials: origin.credentials.as_ref(),
            challenge: None,
        };
        let origin = &origin.uri;
        let description =
            wire.exchange("DESCRIBE", origin, "Accept: application/sdp\r\n", limit)?;
        let uris = ControlUris::from_description(origin, &description)?;
        let sdp = std::str::from_utf8(&description.body).map_err(|_| Error::InvalidResponse)?;
        let (aggregate, mut tracks) = parse_track(&uris, sdp)?;
        let mut datagram = transport == Transport::Udp;
        let (video, session, keepalive) =
            match setup(&mut wire, &tracks.video.control, "", 0, datagram, limit) {
                Err(Error::UnsupportedTransport) if transport == Transport::PreferTcp => {
                    datagram = true;
                    setup(&mut wire, &tracks.video.control, "", 0, true, limit)?
                }
                other => other?,
            };
        let mut connection = Self {
            wire,
            aggregate,
            session,
            media: [Some(video), None],
            datagram,
            next_udp_track: 0,
            next_control_poll: Instant::now(),
            keepalive,
            next_keepalive: Instant::now() + keepalive,
            pending: None,
        };
        if let Some(audio) = &tracks.audio {
            let (audio, _, _) = setup(
                &mut connection.wire,
                &audio.control,
                &connection.session,
                2,
                datagram,
                limit,
            )?;
            if let (Some(MediaTransport::Interleaved(video)), MediaTransport::Interleaved(audio)) =
                (&connection.media[0], &audio)
                && video.abs_diff(*audio) < 2
            {
                return Err(Error::InvalidResponse);
            }
            connection.media[1] = Some(audio);
        }
        let reply =
            connection
                .wire
                .exchange("PLAY", &connection.aggregate, &connection.session, limit)?;
        if let Some(audio) = &tracks.audio
            && let Some(header) = reply.field("RTP-Info")?
            && let (Some(video), Some(audio)) = (
                rtp_clock(header, &tracks.video.control, &uris),
                rtp_clock(header, &audio.control, &uris),
            )
        {
            tracks.start_clocks = Some((video, audio));
        }
        if datagram {
            connection
                .wire
                .stream
                .get_ref()
                .set_read_timeout(Some(Duration::from_millis(3)))
                .map_err(|_| Error::Unavailable)?;
        }
        Ok((connection, tracks))
    }

    pub fn next_rtp(&mut self, media_deadline: Instant) -> Result<Option<MediaPacket>, Error> {
        let now = Instant::now();
        check_open(self.wire.closed)?;
        if let Some(pending) = self.pending {
            check_deadline(pending.deadline)?;
        } else if now >= self.next_keepalive {
            let cseq = self
                .wire
                .request("OPTIONS", &self.aggregate, &self.session)?;
            self.pending = Some(PendingKeepalive {
                cseq,
                deadline: now + REQUEST_TIMEOUT,
                retried: false,
            });
            self.next_keepalive = now + self.keepalive;
        }
        // Poll both sockets fairly and service control periodically even during a burst.
        if self.datagram && now < self.next_control_poll {
            let mut buffer = [0; 65536];
            for offset in 0..2 {
                let track = (self.next_udp_track + offset) % 2;
                if let Some(MediaTransport::Datagram(pair)) = &self.media[track]
                    && let Some(size) = udp::receive(&pair.rtp, &mut buffer)?
                {
                    self.next_udp_track = (track + 1) % 2;
                    return Ok(Some(MediaPacket {
                        track,
                        bytes: buffer[..size].to_vec(),
                    }));
                }
            }
        }
        self.next_control_poll = now + Duration::from_millis(20);
        // A single poll handles one frame so unrelated traffic cannot defer the media watchdog.
        let deadline = self
            .pending
            .map_or(now + REQUEST_TIMEOUT, |pending| pending.deadline)
            .min(media_deadline);
        match self.wire.next(deadline)? {
            Some(Frame::Interleaved(channel, bytes)) => Ok(self.media.iter().position(|transport| {
                matches!(transport, Some(MediaTransport::Interleaved(rtp)) if *rtp == channel)
            }).map(|track| MediaPacket { track, bytes })),
            Some(Frame::Message(message)) => {
                if let Some(status) = message.status()? {
                    let cseq = message.cseq()?;
                    if let Some(mut pending) = self.pending
                        && pending.cseq == cseq
                    {
                        if status == 401 && !pending.retried {
                            check_deadline(deadline)?;
                            self.wire.update_auth_challenge(&message)?;
                            pending.cseq = self.wire.request("OPTIONS", &self.aggregate, &self.session)?;
                            pending.retried = true;
                            self.pending = Some(pending);
                        } else {
                            response_status(status)?;
                            self.pending = None;
                        }
                    }
                } else {
                    self.wire.reply_to_server(&message)?;
                }
                Ok(None)
            }
            _ => Ok(None),
        }
    }
}

impl Drop for Session<'_> {
    fn drop(&mut self) {
        let _ = self
            .wire
            .request("TEARDOWN", &self.aggregate, &self.session);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufRead;
    use std::net::TcpListener;

    #[test]
    fn track_control_stays_on_the_selected_origin() {
        let origin = parse_uri("rtsp://127.0.0.1:8554/live").expect("origin");
        let description = Message {
            header: "RTSP/1.0 200 OK\r\nContent-Base: /live/\r\n\r\n".into(),
            body: Vec::new(),
        };
        let uris = ControlUris::from_description(&origin, &description).expect("base");
        let sdp = "v=0\r\na=control:*\r\nm=video 0 RTP/AVP 96\r\na=rtpmap:96 H264/90000\r\na=control:trackID=0\r\nm=audio 0 RTP/AVP 97\r\n";
        let (_, track) = parse_track(&uris, sdp).expect("track");
        assert_eq!(
            track.video.control.as_str(),
            "rtsp://127.0.0.1:8554/live/trackID=0"
        );
        assert!(track.audio_omitted);
        let foreign = sdp.replace("trackID=0", "rtsp://127.0.0.1:9554/foreign");
        assert!(matches!(
            parse_track(&uris, &foreign),
            Err(Error::UnsupportedInput)
        ));
    }

    #[test]
    fn implicit_media_base_preserves_selectors_without_rewriting_the_aggregate_uri() {
        let origin =
            parse_uri("rtsp://127.0.0.1:8554/cam/realmonitor?channel=1&subtype=0").expect("origin");
        let description = Message {
            header: "RTSP/1.0 200 OK\r\n\r\n".into(),
            body: Vec::new(),
        };
        let uris = ControlUris::from_description(&origin, &description).expect("base");
        let sdp = "v=0\r\na=control:*\r\nm=video 0 RTP/AVP 96\r\na=rtpmap:96 H264/90000\r\na=control:trackID=0\r\n";
        let (aggregate, tracks) = parse_track(&uris, sdp).expect("track");
        assert_eq!(aggregate, origin);
        assert_eq!(
            tracks.video.control.as_str(),
            "rtsp://127.0.0.1:8554/cam/realmonitor/trackID=0?channel=1&subtype=0"
        );
        assert_eq!(
            rtp_clock("url=trackID=0;rtptime=42", &tracks.video.control, &uris),
            Some(42)
        );
        assert_eq!(
            uris.track("trackID=1?channel=2")
                .expect("explicit query")
                .query(),
            Some("channel=2")
        );
        assert_eq!(uris.track("/other").expect("absolute path").query(), None);
        let description = Message {
            header: "RTSP/1.0 200 OK\r\nContent-Location: /redirected/\r\n\r\n".into(),
            body: Vec::new(),
        };
        let uris = ControlUris::from_description(&origin, &description).expect("content location");
        let (aggregate, tracks) = parse_track(&uris, sdp).expect("redirected track");
        assert_eq!(aggregate.path(), "/redirected/");
        assert_eq!(tracks.video.control.path(), "/redirected/trackID=0");
        assert_eq!(tracks.video.control.query(), None);
    }

    #[test]
    fn parameter_set_lists_are_written_to_avcc_and_hvcc() {
        for (codec, format, box_type, first_pps, second_pps) in [
            (
                "H264",
                "sprop-parameter-sets=aM4PyA==,Z0LAHtoCgL/lwEQAAAMABAAAAwDyPFi6gA==,aFOD8g==",
                b"avcC",
                "aM4PyA==",
                "aFOD8g==",
            ),
            (
                "H265",
                "sprop-vps=QAEMAf//AWAAAAMAkAAAAwAAAwA/lZgJ;sprop-sps=QgEBAWAAAAMAkAAAAwAAAwA/oAUCAWlllZpJMrwFoCAAAAMAIAAAAwPB;sprop-pps=RAHBcrQiQA==,RAFQXK0IkA==",
                b"hvcC",
                "RAHBcrQiQA==",
                "RAFQXK0IkA==",
            ),
        ] {
            let origin = parse_uri("rtsp://127.0.0.1:8554/live").expect("origin");
            let description = Message {
                header: "RTSP/1.0 200 OK\r\n\r\n".into(),
                body: Vec::new(),
            };
            let uris = ControlUris::from_description(&origin, &description).expect("base");
            let sdp = format!(
                "v=0\r\nm=video 0 RTP/AVP 96\r\na=rtpmap:96 {codec}/90000\r\na=fmtp:96 {format}\r\na=control:trackID=0\r\n"
            );
            let (_, tracks) = parse_track(&uris, &sdp).expect("multi-parameter-set SDP");
            let init = crate::mp4::init(&tracks.video.config, None).expect("fMP4 initialization");
            let box_end = init
                .windows(4)
                .position(|kind| kind == box_type)
                .expect("decoder configuration");
            let box_size =
                u32::from_be_bytes(init[box_end - 4..box_end].try_into().expect("size")) as usize;
            let mut records = if codec == "H264" {
                vec![2]
            } else {
                vec![0xa2, 0, 2]
            };
            for pps in [first_pps, second_pps] {
                let nal = STANDARD.decode(pps).expect("PPS");
                records.extend((nal.len() as u16).to_be_bytes());
                records.extend(nal);
            }
            assert!(
                init[box_end + 4..box_end - 4 + box_size]
                    .windows(records.len())
                    .any(|window| window == records)
            );
        }
    }

    #[test]
    fn single_sps_in_band_resolution_change_can_advance_to_a_new_id() {
        const SPS: &str = "Z0LAHtoCgL/lwEQAAAMABAAAAwDyPFi6gA==";
        let mut config = VideoConfig::default();
        parse_parameter_sets(&mut config, &format!("{SPS},aM4PyA=="), None)
            .expect("initial SDP configuration");
        let mut assembler = crate::rtp::VideoAssembler::new(config);
        let sps = STANDARD
            .decode("Z0LADVZAUGfnwEQAAAMABAAAAwDwPFCpIA==")
            .expect("new SPS");
        let pps = STANDARD.decode("aEi4PLI=").expect("new PPS");
        for (sequence, payload) in [(1, sps.as_slice()), (2, pps.as_slice())] {
            assert!(
                assembler
                    .push(&crate::rtp::Packet {
                        payload_type: 96,
                        marker: true,
                        sequence,
                        timestamp: 0,
                        ssrc: 1,
                        payload,
                    })
                    .is_none()
            );
        }
        assert_eq!(
            assembler.config.sps.keys().copied().collect::<Vec<_>>(),
            [1]
        );
        assert_eq!(
            assembler.config.pps.keys().copied().collect::<Vec<_>>(),
            [1]
        );
        assert!(crate::mp4::init(&assembler.config, None).is_ok());
    }

    #[test]
    fn keepalive_renews_a_stale_digest_nonce_without_discarding_interleaved_media() {
        for status in [200, 401] {
            let listener = TcpListener::bind("127.0.0.1:0").expect("origin listener");
            let address = listener.local_addr().expect("origin address");
            let origin =
                parse_origin(&format!("rtsp://camera:secret@{address}/live")).expect("origin");
            let closed = AtomicBool::new(false);
            let stream = connect(&origin.uri, &closed, None).expect("control connection");
            let mut server = BufReader::new(listener.accept().expect("client connection").0);
            server
                .get_ref()
                .set_read_timeout(Some(REQUEST_TIMEOUT))
                .expect("test deadline");
            let now = Instant::now();
            let mut session = Session {
                wire: Wire {
                    stream: BufReader::new(stream),
                    closed: &closed,
                    cseq: 3,
                    credentials: origin.credentials.as_ref(),
                    challenge: Challenge::select(std::iter::once(
                        "Digest realm=\"Camera\", nonce=\"old\", qop=\"auth\"",
                    )),
                },
                aggregate: origin.uri,
                session: "Session: test\r\n".into(),
                media: [Some(MediaTransport::Interleaved(0)), None],
                datagram: false,
                next_udp_track: 0,
                next_control_poll: now,
                keepalive: Duration::from_secs(30),
                next_keepalive: now,
                pending: None,
            };
            let stale = "WWW-Authenticate: Digest realm=\"Camera\", nonce=\"new\", qop=\"auth\", stale=true\r\n";
            write!(
                server.get_mut(),
                "RTSP/1.0 401 Unauthorized\r\nCSeq: 4\r\n{stale}\r\n"
            )
            .expect("stale challenge");
            server
                .get_mut()
                .write_all(b"$\x00\x00\x03rtp")
                .expect("interleaved media");
            write!(
                server.get_mut(),
                "RTSP/1.0 {status} Reply\r\nCSeq: 5\r\n{stale}\r\n"
            )
            .expect("retry response");

            let deadline = now + REQUEST_TIMEOUT;
            assert!(session.next_rtp(deadline).expect("refresh nonce").is_none());
            let packet = session
                .next_rtp(deadline)
                .expect("continued playback")
                .expect("RTP");
            assert_eq!(
                (packet.track, packet.bytes.as_slice()),
                (0, b"rtp".as_slice())
            );
            let response = session.next_rtp(deadline);
            if status == 200 {
                assert!(response.expect("successful keepalive").is_none());
                assert!(session.pending.is_none());
            } else {
                assert!(matches!(response, Err(Error::AuthenticationRequired)));
            }
            assert_eq!(session.wire.cseq, 5, "at most one retry per keepalive");
            for (cseq, nonce) in [(4, "old"), (5, "new")] {
                let mut request = String::new();
                for _ in 0..6 {
                    let mut line = String::new();
                    assert_ne!(server.read_line(&mut line).expect("read OPTIONS"), 0);
                    if line == "\r\n" {
                        break;
                    }
                    request.push_str(&line);
                }
                assert!(request.starts_with(&format!(
                    "OPTIONS rtsp://{address}/live RTSP/1.0\r\nCSeq: {cseq}\r\n"
                )));
                assert!(request.contains(&format!("nonce=\"{nonce}\"")));
                assert!(request.contains("nc=00000001"));
                assert!(!request.contains("secret"));
            }
        }
    }
}
