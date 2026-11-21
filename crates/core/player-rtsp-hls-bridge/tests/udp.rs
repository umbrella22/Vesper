use player_rtsp_hls_bridge::{Bridge, Phase};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream, UdpSocket};
use std::thread;
use std::time::{Duration, Instant};
use url::Url;

#[test]
fn udp_fallback_discards_a_damaged_gop_and_publishes_a_discontinuity() {
    let origin = TcpListener::bind("127.0.0.1:0").expect("RTSP listener");
    let address = origin.local_addr().expect("RTSP address");
    origin.set_nonblocking(true).expect("nonblocking accept");
    let bridge = Bridge::start(&format!("rtsp://{address}/clock")).expect("bridge");
    let socket = wait_for(|| origin.accept().ok()).0;
    socket.set_nonblocking(false).expect("blocking peer");
    socket
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("RTSP timeout");
    let mut control = BufReader::new(socket);
    let sdp = concat!(
        "v=0\r\na=control:*\r\nm=video 0 RTP/AVP 96\r\n",
        "a=rtpmap:96 H264/90000\r\na=control:trackID=0\r\n",
        "a=fmtp:96 packetization-mode=1; sprop-parameter-sets=",
        "Z0LAHtoCgL/lhAAAAwAEAAADAPI8WLqA,aM4PyA==\r\n",
    );
    assert!(request(&mut control).starts_with("DESCRIBE "));
    write!(
        control.get_mut(),
        "RTSP/1.0 200 OK\r\nCSeq: 1\r\nContent-Length: {}\r\n\r\n{sdp}",
        sdp.len()
    )
    .expect("SDP");
    assert!(request(&mut control).contains("Transport: RTP/AVP/TCP;unicast"));
    control
        .get_mut()
        .write_all(b"RTSP/1.0 461 Unsupported Transport\r\nCSeq: 2\r\n\r\n")
        .expect("reject TCP transport");
    let setup = request(&mut control);
    let ports = setup
        .lines()
        .find_map(|line| line.strip_prefix("Transport: RTP/AVP;unicast;client_port="))
        .expect("UDP port pair");
    let rtp = UdpSocket::bind("127.0.0.1:0").expect("RTP socket");
    let rtcp = UdpSocket::bind("127.0.0.1:0").expect("RTCP socket");
    let rtp_port = rtp.local_addr().expect("RTP address").port();
    let rtcp_port = rtcp.local_addr().expect("RTCP address").port();
    write!(control.get_mut(), "RTSP/1.0 200 OK\r\nCSeq: 3\r\nSession: test\r\nTransport: RTP/AVP;unicast;client_port={ports};server_port={rtp_port}-{rtcp_port}\r\n\r\n")
        .expect("negotiate UDP");
    assert!(request(&mut control).starts_with("PLAY "));
    control
        .get_mut()
        .write_all(b"RTSP/1.0 200 OK\r\nCSeq: 4\r\nSession: test\r\n\r\n")
        .expect("start RTP");
    let (client_port, _) = ports.split_once('-').expect("client port pair");
    rtp.connect(format!("127.0.0.1:{client_port}"))
        .expect("RTP client");
    let send = |seq: u16, ts: u32, marker, payload: &[u8]| {
        let mut packet = vec![0x80, if marker { 224 } else { 96 }];
        packet.extend(seq.to_be_bytes());
        packet.extend(ts.to_be_bytes());
        packet.extend(1_u32.to_be_bytes());
        packet.extend(payload);
        assert_eq!(rtp.send(&packet).expect("local datagram"), packet.len());
    };
    for seq in 0..=2 {
        send(seq, u32::from(seq) * 90_000, true, &[0x65, 0xb0]);
    }
    wait_for(|| (bridge.snapshot().phase == Phase::Ready).then_some(()));

    send(3, 183_000, false, &[0x7c, 0x81, 0xc0]);
    send(5, 183_000, true, &[0x7c, 0x41, 2]);
    send(6, 186_000, true, &[0x61, 0xc0]);
    for seq in 7..=9 {
        send(seq, u32::from(seq - 4) * 90_000, true, &[0x65, 0xb0]);
    }
    let playlist = wait_for(|| {
        let playlist = fetch(bridge.playlist_url());
        playlist.contains("seg-3.m4s").then_some(playlist)
    });
    assert!(playlist.contains("#EXT-X-DISCONTINUITY\n#EXT-X-MAP:URI=\"init-2.mp4\""));
    let snapshot = bridge.snapshot();
    assert_eq!(snapshot.phase, Phase::Ready);
    assert_eq!(snapshot.damaged_access_units, 1);
    assert_eq!(snapshot.reconnects, 0);
}

fn wait_for<T>(mut observation: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if let Some(observed) = observation() {
            return observed;
        }
        assert!(Instant::now() < deadline, "RTSP test observation timed out");
        thread::sleep(Duration::from_millis(10));
    }
}

fn request(control: &mut BufReader<TcpStream>) -> String {
    let mut headers = String::new();
    for _ in 0..12 {
        let mut line = String::new();
        assert_ne!(control.read_line(&mut line).expect("RTSP request"), 0);
        if line == "\r\n" {
            return headers;
        }
        headers.push_str(&line);
    }
    panic!("RTSP request exceeded the fixture header limit");
}

fn fetch(playlist: &str) -> String {
    let url = Url::parse(playlist).expect("playlist URL");
    let host = format!("127.0.0.1:{}", url.port().expect("HTTP port"));
    let mut http = TcpStream::connect(&host).expect("playlist HTTP listener");
    http.set_read_timeout(Some(Duration::from_secs(1)))
        .expect("HTTP timeout");
    write!(
        http,
        "GET {} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n",
        url.path()
    )
    .expect("playlist request");
    let mut response = String::new();
    http.take(8192)
        .read_to_string(&mut response)
        .expect("playlist response");
    assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
    response
}
