use std::io::{BufRead, BufReader, ErrorKind, Read, Write};
use std::net::TcpListener;
use std::thread;
use std::time::Duration;

use player_model::MediaSource;
use player_platform_desktop::download::prepare_download_task;
use player_runtime::{PlayerError, PlayerErrorCode};

const HEAD_ASSET: &str = "HEAD /asset.mp4 HTTP/1.1";
const HEAD_TARGET: &str = "HEAD /target.mp4 HTTP/1.1";
const GET_ASSET: &str = "GET /asset.mp4 HTTP/1.1";
const GET_TARGET: &str = "GET /target.mp4 HTTP/1.1";
const REDIRECT: &str = "302 Found\r\nLocation: /target.mp4\r\nContent-Length: 0\r\n\r\n";
const UNKNOWN_HEAD: &str = "200 OK\r\n\r\n";

#[test]
#[ignore = "requires curl and a loopback HTTP listener"]
fn redirect_head_length_does_not_mask_range_fallback() {
    let size = plan_size(&[
        (HEAD_ASSET, REDIRECT),
        (HEAD_TARGET, UNKNOWN_HEAD),
        (GET_ASSET, REDIRECT),
        (
            GET_TARGET,
            "206 Partial Content\r\nContent-Length: 1\r\nContent-Range: bytes 0-0/73\r\n\r\na",
        ),
    ])
    .unwrap();
    assert_eq!(size, Some(73));
}

#[test]
#[ignore = "requires curl and a loopback HTTP listener"]
fn redirect_range_total_does_not_supply_missing_final_total() {
    let error = plan_size(&[
        (HEAD_ASSET, UNKNOWN_HEAD),
        (
            GET_ASSET,
            "302 Found\r\nLocation: /target.mp4\r\nContent-Length: 0\r\nContent-Range: bytes 0-0/99\r\n\r\n",
        ),
        (GET_TARGET, "206 Partial Content\r\nContent-Length: 1\r\n\r\na"),
    ])
    .unwrap_err();
    let error = error.downcast_ref::<PlayerError>().unwrap();
    assert_eq!(error.code(), PlayerErrorCode::InvalidSource);
    assert!(
        error
            .message()
            .contains("did not expose a stable content length")
    );
}

#[test]
#[ignore = "requires curl and a loopback HTTP listener"]
fn final_head_length_of_zero_is_valid() {
    let size = plan_size(&[
        (HEAD_ASSET, REDIRECT),
        (HEAD_TARGET, "200 OK\r\nContent-Length: 0\r\n\r\n"),
    ])
    .unwrap();
    assert_eq!(size, Some(0));
}

fn plan_size(replies: &[(&str, &str)]) -> anyhow::Result<Option<u64>> {
    let origin = TcpListener::bind("127.0.0.1:0").unwrap();
    origin.set_nonblocking(true).unwrap();
    let source = MediaSource::new(format!("http://{}/asset.mp4", origin.local_addr().unwrap()));
    thread::scope(move |scope| {
        let planner = scope.spawn(move || {
            prepare_download_task("length-probe", &source, "Length probe")
                .map(|prepared| prepared.asset_index.total_size_bytes)
        });
        let mut replies = replies.iter().copied();
        while !planner.is_finished() {
            let (mut stream, _) = match origin.accept() {
                Ok(connection) => connection,
                Err(error) if error.kind() == ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(5));
                    continue;
                }
                Err(error) => panic!("loopback accept failed: {error}"),
            };
            let (expected_request, response) = replies.next().expect("unexpected request");
            stream.set_nonblocking(false).unwrap();
            let timeout = Some(Duration::from_secs(2));
            stream.set_read_timeout(timeout).unwrap();
            stream.set_write_timeout(timeout).unwrap();

            let mut lines = BufReader::new(&stream).take(4096).lines();
            assert_eq!(lines.next().unwrap().unwrap(), expected_request);
            let mut has_range = false;
            for line in lines {
                let line = line.unwrap();
                if line.is_empty() {
                    break;
                }
                has_range |= line.eq_ignore_ascii_case("Range: bytes=0-0");
            }
            assert_eq!(has_range, expected_request.starts_with("GET "));
            let (status, headers_and_body) = response.split_once("\r\n").unwrap();
            write!(
                stream,
                "HTTP/1.1 {status}\r\nConnection: close\r\n{headers_and_body}"
            )
            .unwrap();
        }
        planner.join().unwrap()
    })
}
