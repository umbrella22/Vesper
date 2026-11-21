use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::ops::Range;
use std::sync::{Arc, atomic::Ordering};
use std::thread;
use std::time::{Duration, Instant};

use crate::{Phase, Shared};

const REQUEST_DEADLINE: Duration = Duration::from_millis(800);
const SOCKET_POLL: Duration = Duration::from_millis(100);
const MAX_HEADER_BYTES: usize = 8192;

pub(crate) fn serve(
    listener: TcpListener,
    address: SocketAddr,
    token: String,
    shared: Arc<Shared>,
) {
    let host = address.to_string();
    let alias = format!("localhost:{}", address.port());
    let path_prefix = format!("/{token}/");
    // One bounded response at a time. Its Arc can outlive eviction, so at most
    // one segment-sized allocation is pinned beyond the window's byte budget.
    while !shared.closed.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((mut socket, peer)) if peer.ip().is_loopback() => {
                let _ = answer(&mut socket, &host, &alias, &path_prefix, &shared);
            }
            Ok(_) => (),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10))
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => (),
            Err(_) => {
                if !shared.closed.load(Ordering::Relaxed) {
                    shared.state().phase = Phase::Failed(crate::BridgeError::LocalHttp);
                    shared.closed.store(true, Ordering::Relaxed);
                }
                break;
            }
        }
    }
}

fn would_poll(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::Interrupted | io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
    )
}

fn read_headers(
    socket: &mut TcpStream,
    deadline: Instant,
    shared: &Shared,
) -> io::Result<Option<String>> {
    let mut header = Vec::new();
    let mut buffer = [0; 1024];
    while header.len() < MAX_HEADER_BYTES
        && Instant::now() < deadline
        && !shared.closed.load(Ordering::Relaxed)
    {
        let previous = header.len();
        match socket.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => {
                header.extend_from_slice(&buffer[..count]);
                if let Some(end) = header[previous.saturating_sub(3)..]
                    .windows(4)
                    .position(|s| s == b"\r\n\r\n")
                {
                    let end = previous.saturating_sub(3) + end + 4;
                    if end <= MAX_HEADER_BYTES {
                        header.truncate(end);
                        return Ok(String::from_utf8(header).ok());
                    }
                    break;
                }
            }
            Err(error) if would_poll(&error) => (),
            Err(error) => return Err(error),
        }
    }
    Ok(None)
}

fn parse_range(value: &str, size: usize) -> Option<Range<usize>> {
    let (start, end) = value.strip_prefix("bytes=")?.split_once('-')?;
    if size == 0 {
        return None;
    }
    if start.is_empty() {
        let suffix = end.parse::<usize>().ok()?;
        return (suffix != 0).then(|| size.saturating_sub(suffix)..size);
    }
    let start = start.parse::<usize>().ok()?;
    let end = if end.is_empty() {
        size - 1
    } else {
        end.parse::<usize>().ok()?.min(size - 1)
    };
    (start <= end).then_some(start..end + 1)
}

fn answer(
    socket: &mut TcpStream,
    host: &str,
    alias: &str,
    prefix: &str,
    shared: &Shared,
) -> io::Result<()> {
    socket.set_read_timeout(Some(SOCKET_POLL))?;
    socket.set_write_timeout(Some(SOCKET_POLL))?;
    let deadline = Instant::now() + REQUEST_DEADLINE;
    let Some(headers) = read_headers(socket, deadline, shared)? else {
        return Ok(());
    };
    let mut lines = headers.lines();
    let mut request = lines.next().unwrap_or("").split_ascii_whitespace();
    let (method, path, version) = (request.next(), request.next(), request.next());
    let mut host_values = Vec::new();
    let mut ranges = Vec::new();
    for (name, value) in lines.filter_map(|line| line.split_once(':')) {
        if name.eq_ignore_ascii_case("Host") {
            host_values.push(value.trim());
        } else if name.eq_ignore_ascii_case("Range") {
            ranges.push(value.trim());
        }
    }
    let head = method == Some("HEAD");
    let valid_host = matches!(host_values.as_slice(), [value] if *value == host || *value == alias);
    let name = path.and_then(|path| path.strip_prefix(prefix));
    let mut response_range = None;
    let mut extra = String::new();
    let mut body: Arc<[u8]> = Arc::from([]);
    let (status, mime) = if !matches!(version, Some("HTTP/1.0" | "HTTP/1.1"))
        || request.next().is_some()
        || !valid_host
    {
        ("400 Bad Request", "text/plain")
    } else if !matches!(method, Some("GET" | "HEAD")) {
        extra.push_str("Allow: GET, HEAD\r\n");
        ("405 Method Not Allowed", "text/plain")
    } else if let Some(name) = name {
        let (status, mime, bytes) = resource(name, shared);
        body = bytes;
        if status == "200 OK" {
            extra.push_str("Accept-Ranges: bytes\r\n");
        }
        if status == "200 OK" && !head && !ranges.is_empty() {
            let size = body.len();
            if let [value] = ranges.as_slice()
                && let Some(range) = parse_range(value, size)
            {
                extra.push_str(&format!(
                    "Content-Range: bytes {}-{}/{size}\r\n",
                    range.start,
                    range.end - 1
                ));
                response_range = Some(range);
                ("206 Partial Content", mime)
            } else {
                extra.push_str(&format!("Content-Range: bytes */{size}\r\n"));
                body = Arc::from([]);
                ("416 Range Not Satisfiable", "text/plain")
            }
        } else {
            (status, mime)
        }
    } else {
        ("404 Not Found", "text/plain")
    };
    let bytes = response_range.map_or(body.as_ref(), |range| &body[range]);
    let header = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n{extra}\r\n",
        bytes.len()
    );
    let deadline = Instant::now() + REQUEST_DEADLINE;
    write_until(socket, header.as_bytes(), deadline, shared)?;
    if !head {
        write_until(socket, bytes, deadline, shared)?;
    }
    Ok(())
}

fn resource(name: &str, shared: &Shared) -> (&'static str, &'static str, Arc<[u8]>) {
    let mut state = shared.state();
    if name == "index.m3u8" {
        if matches!(state.phase, Phase::Failed(_)) {
            return ("410 Gone", "text/plain", Arc::from([]));
        }
        if let Some(playlist) = state.window.playlist() {
            return (
                "200 OK",
                "application/vnd.apple.mpegurl",
                Arc::from(playlist.into_bytes()),
            );
        }
        return ("503 Service Unavailable", "text/plain", Arc::from([]));
    }
    if let Some(bytes) = state.window.resource(name) {
        return ("200 OK", "video/mp4", bytes);
    }
    ("404 Not Found", "text/plain", Arc::from([]))
}

fn write_until(
    socket: &mut TcpStream,
    mut bytes: &[u8],
    deadline: Instant,
    shared: &Shared,
) -> io::Result<()> {
    while !bytes.is_empty() {
        if Instant::now() >= deadline || shared.closed.load(Ordering::Relaxed) {
            return Err(io::ErrorKind::TimedOut.into());
        }
        match socket.write(bytes) {
            Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
            Ok(count) => bytes = &bytes[count..],
            Err(error) if would_poll(&error) => (),
            Err(error) => return Err(error),
        }
    }
    Ok(())
}
