use player_rtsp_hls_bridge::{Bridge, BridgeError, Phase};
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};
use url::Url;

#[test]
fn unauthorized_describe_is_terminal_and_drop_releases_the_loopback_listener() {
    let origin = TcpListener::bind("127.0.0.1:0").expect("origin listener");
    origin.set_nonblocking(true).expect("nonblocking listener");
    let address = origin.local_addr().expect("origin address");
    let bridge =
        Bridge::start(&format!("rtsp://{address}/clock?token=fixture-secret")).expect("start");
    let local = Url::parse(bridge.playlist_url()).expect("local capability URL");
    let local = format!("127.0.0.1:{}", local.port().expect("local port"));
    assert!(TcpStream::connect(&local).is_ok());

    let deadline = Instant::now() + Duration::from_secs(2);
    let socket = loop {
        if let Ok((socket, _)) = origin.accept() {
            break socket;
        }
        assert!(
            Instant::now() < deadline,
            "bridge never connected to origin"
        );
        std::thread::sleep(Duration::from_millis(10));
    };
    socket
        .set_nonblocking(false)
        .expect("blocking accepted socket");
    socket
        .set_read_timeout(Some(Duration::from_secs(1)))
        .expect("test timeout");
    let mut reader = BufReader::new(socket);
    let mut first = String::new();
    reader.read_line(&mut first).expect("request");
    assert!(first.starts_with(&format!(
        "DESCRIBE rtsp://{address}/clock?token=fixture-secret RTSP/1.0"
    )));
    let mut header = String::new();
    reader.read_line(&mut header).expect("CSeq");
    assert_eq!(header, "CSeq: 1\r\n");
    reader
        .get_mut()
        .write_all(b"RTSP/1.0 401 Unauthorized\r\nCSeq: 1\r\nContent-Length: 0\r\n\r\n")
        .expect("authentication response");
    let deadline = Instant::now() + Duration::from_secs(2);
    let failure = loop {
        if let Phase::Failed(failure) = bridge.snapshot().phase {
            break failure;
        }
        assert!(
            Instant::now() < deadline,
            "bridge never reported authorization failure"
        );
        std::thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(failure, BridgeError::AuthenticationRequired);
    assert!(!failure.to_string().contains("fixture-secret"));
    assert_eq!(bridge.snapshot().reconnects, 0);
    drop(bridge);

    let deadline = Instant::now() + Duration::from_secs(2);
    while TcpStream::connect(&local).is_ok() {
        assert!(
            Instant::now() < deadline,
            "the dropped bridge still accepts requests"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn digest_stale_challenge_is_retried_with_redacted_request_targets() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("RTSP listener");
    let address = listener.local_addr().expect("RTSP address");
    let uri = format!("rtsp://camera:fixture%40secret@{address}/clock");
    let bridge = Bridge::start(&uri).expect("start credentialed bridge");
    listener
        .set_nonblocking(true)
        .expect("nonblocking listener");
    let deadline = Instant::now() + Duration::from_secs(2);
    let (socket, _) = loop {
        match listener.accept() {
            Ok(result) => break result,
            Err(error) => {
                assert!(
                    Instant::now() < deadline,
                    "RTSP connection never opened: {error}"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    };
    socket
        .set_nonblocking(false)
        .expect("blocking accepted socket");
    socket
        .set_read_timeout(Some(Duration::from_secs(1)))
        .expect("test request deadline");
    let mut reader = BufReader::new(socket);
    for (sequence, nonce, response) in [
        (
            1,
            None,
            "401 Unauthorized\r\nWWW-Authenticate: Basic realm=\"Clock\"\r\nWWW-Authenticate: Digest realm=\"Clock\", nonce=\"nonce-a\", algorithm=SHA-256",
        ),
        (
            2,
            Some("nonce-a"),
            "401 Unauthorized\r\nWWW-Authenticate: Digest realm=\"Clock\", nonce=\"nonce-b\", algorithm=SHA-256, stale=true",
        ),
        (3, Some("nonce-b"), "403 Forbidden"),
    ] {
        let mut request = String::new();
        for _ in 0..16 {
            let mut line = String::new();
            let bytes = reader.read_line(&mut line).expect("bounded request read");
            assert_ne!(bytes, 0);
            if line == "\r\n" {
                break;
            }
            request.push_str(&line);
        }
        assert!(request.starts_with(&format!(
            "DESCRIBE rtsp://{address}/clock RTSP/1.0\r\nCSeq: {sequence}\r\n"
        )));
        assert!(!request.contains("fixture"));
        if let Some(nonce) = nonce {
            assert!(request.contains("Authorization: Digest username=\"camera\""));
            assert!(request.contains(&format!("nonce=\"{nonce}\"")));
            assert!(request.contains("algorithm=SHA-256"));
        } else {
            assert!(!request.contains("Authorization:"));
        }
        write!(
            reader.get_mut(),
            "RTSP/1.0 {response}\r\nCSeq: {sequence}\r\n\r\n"
        )
        .expect("mock challenge response");
    }
    let deadline = Instant::now() + Duration::from_secs(2);
    while bridge.snapshot().phase != Phase::Failed(BridgeError::AuthenticationRequired) {
        assert!(
            Instant::now() < deadline,
            "authorization refusal was not terminal"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(bridge.snapshot().reconnects, 0);
}
