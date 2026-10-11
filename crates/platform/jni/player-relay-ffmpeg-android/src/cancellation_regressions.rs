use super::*;
use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;

fn fixture() -> (PathBuf, OpenRequest, Arc<GrowingCache>) {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let root = std::env::temp_dir().join(format!(
        "vesper-relay-cancel-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("segment_00000.ts"), b"unread media bytes").unwrap();
    fs::write(
        root.join("playlist.m3u8"),
        b"#EXTM3U\n#EXTINF:4,\nsegment_00000.ts\n",
    )
    .unwrap();
    let cache = Arc::new(GrowingCache::new(root.join("playlist.m3u8")));
    cache.mark_complete();
    let request = OpenRequest {
        session_id: root.file_name().unwrap().to_string_lossy().into_owned(),
        session_handle: 0,
        input_mode: Some(HOST_PREPARED_DASH_INPUT_MODE.into()),
        tracks: Vec::new(),
        source_uri_hash: None,
        source_label: None,
        fallback_format: FallbackFormat::Hls,
        resource_path: "playlist.m3u8".into(),
        range: None,
        enable_range_cache: true,
        debug_diagnostics: false,
        route_id: None,
        route_name: None,
    };
    (root, request, cache)
}

#[test]
fn cancelled_initial_bytes_win_over_complete_data() {
    let (root, request, cache) = fixture();
    cache.mark_cancelled();
    let result = wait_for_initial_cache_bytes(&request, &cache, Duration::ZERO);
    assert!(matches!(result, Err(error) if error.code == "host_input_cancelled"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn cancelled_range_wins_over_complete_data() {
    let (root, request, cache) = fixture();
    cache.mark_cancelled();
    let result = wait_for_cache_range(
        &request,
        &cache,
        RangeRequest {
            start: Some(0),
            end: Some(3),
        },
        Duration::ZERO,
    );
    assert!(matches!(result, Err(error) if error.code == "host_input_cancelled"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn cancelled_hls_playlist_wins_over_complete_data() {
    let (root, request, cache) = fixture();
    cache.mark_cancelled();
    let result = wait_for_hls_playlist_snapshot(&request, &root, &cache, Duration::ZERO);
    assert!(matches!(result, Err(error) if error.code == "host_input_cancelled"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn cancelled_hls_segment_wins_over_ready_file() {
    let (root, request, cache) = fixture();
    cache.mark_cancelled();
    let result = wait_for_hls_segment(
        &request,
        &cache,
        &root.join("segment_00000.ts"),
        "segment_00000.ts",
    );
    assert!(matches!(result, Err(error) if error.code == "host_input_cancelled"));
    fs::remove_dir_all(root).unwrap();
}

fn hls_handle_cancel(range: bool, playlist: bool) {
    let (root, mut request, cache) = fixture();
    if !playlist {
        request.resource_path = "segment_00000.ts".into();
    }
    if range {
        request.range = Some(RangeRequest {
            start: Some(0),
            end: Some(3),
        });
    }
    let opened = open_hls_cache_resource(&request, &root, cache.clone()).unwrap();
    cache.mark_cancelled();
    let entry = lock_or_recover(streams())
        .get(opened.handle)
        .unwrap()
        .clone();
    let outcome = lock_or_recover(&entry).read_outcome(&mut [0; 4]);
    lock_or_recover(streams()).remove(opened.handle);
    fs::remove_dir_all(root).unwrap();
    assert!(
        matches!(outcome, NativeReadOutcome::Cancelled),
        "{outcome:?}"
    );
}

#[test]
fn hls_playlist_handle_retains_cancellation() {
    hls_handle_cancel(false, true);
}

#[test]
fn hls_segment_handle_retains_cancellation() {
    hls_handle_cancel(false, false);
}

#[test]
fn hls_range_handle_retains_cancellation() {
    hls_handle_cancel(true, false);
}

#[test]
fn growing_handle_cancel_wins_over_buffered_bytes() {
    let (root, request, cache) = fixture();
    let opened = open_growing_cache_file(&request, cache.clone(), "video/mp2t").unwrap();
    cache.mark_cancelled();
    let entry = lock_or_recover(streams())
        .get(opened.handle)
        .unwrap()
        .clone();
    let outcome = lock_or_recover(&entry).read_outcome(&mut [0; 4]);
    lock_or_recover(streams()).remove(opened.handle);
    fs::remove_dir_all(root).unwrap();
    assert!(
        matches!(outcome, NativeReadOutcome::Cancelled),
        "{outcome:?}"
    );
}

#[cfg(unix)]
#[test]
fn cancelled_worker_exits_fifo_open_without_writer() {
    initialize_ffmpeg().unwrap();
    let (root, mut request, _) = fixture();
    let fifo = root.join("track.fifo");
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    request.tracks = vec![PreparedTrack {
        kind: "audio".into(),
        pipe_path: fifo.to_string_lossy().into_owned(),
        media_id: "audio".into(),
        mime_type: Some("audio/mp4".into()),
        codecs: Some("mp4a.40.2".into()),
    }];
    let cache = Arc::new(GrowingCache::new(root.join("worker.ts")));
    spawn_remux_worker(request, cache.clone(), OutputKind::MpegTs).unwrap();
    while !cache.path.exists() {
        thread::sleep(Duration::from_millis(2));
    }
    thread::sleep(Duration::from_millis(20));
    cache.mark_cancelled();
    let deadline = Instant::now() + Duration::from_millis(500);
    while !lock_or_recover(&cache.worker)
        .as_ref()
        .unwrap()
        .is_finished()
        && Instant::now() < deadline
    {
        thread::sleep(Duration::from_millis(5));
    }
    let exited = lock_or_recover(&cache.worker)
        .as_ref()
        .unwrap()
        .is_finished();
    // Release the baseline's blocking POSIX open before reporting a failed test.
    if !exited {
        drop(fs::OpenOptions::new().write(true).open(&fifo).unwrap());
    }
    cache.join_worker();
    fs::remove_dir_all(root).unwrap();
    assert!(
        exited,
        "cancel must interrupt the actual FIFO worker, not only its consumers"
    );
}

#[cfg(unix)]
#[test]
fn cancelled_worker_exits_fifo_refill_after_data() {
    initialize_ffmpeg().unwrap();
    let (root, mut request, _) = fixture();
    let fifo = root.join("track.fifo");
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    request.tracks = vec![PreparedTrack {
        kind: "audio".into(),
        pipe_path: fifo.to_string_lossy().into_owned(),
        media_id: "audio".into(),
        mime_type: Some("audio/mp4".into()),
        codecs: Some("mp4a.40.2".into()),
    }];
    let cache = Arc::new(GrowingCache::new(root.join("worker.ts")));
    let (writer_ready, writer_ready_rx) = mpsc::channel();
    let (release_writer, release_writer_rx) = mpsc::channel();
    let writer_fifo = fifo.clone();
    let writer = thread::spawn(move || {
        let mut writer = fs::OpenOptions::new()
            .write(true)
            .open(writer_fifo)
            .unwrap();
        const FRAME: [u8; 11] = [255, 241, 76, 64, 1, 127, 252, 1, 24, 32, 7];
        for _ in 0..200 {
            writer.write_all(&FRAME).unwrap();
        }
        writer_ready.send(()).unwrap();
        let _ = release_writer_rx.recv_timeout(Duration::from_secs(2));
    });

    spawn_remux_worker(request, cache.clone(), OutputKind::MpegTs).unwrap();
    assert!(writer_ready_rx.recv_timeout(Duration::from_secs(5)).is_ok());
    let deadline = Instant::now() + Duration::from_millis(200);
    while !cache.path.exists() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(2));
    }
    thread::sleep(Duration::from_millis(25));
    cache.mark_cancelled();
    let deadline = Instant::now() + Duration::from_millis(500);
    while !lock_or_recover(&cache.worker)
        .as_ref()
        .unwrap()
        .is_finished()
        && Instant::now() < deadline
    {
        thread::sleep(Duration::from_millis(5));
    }
    let exited = lock_or_recover(&cache.worker)
        .as_ref()
        .unwrap()
        .is_finished();
    let _ = release_writer.send(());
    writer.join().unwrap();
    cache.join_worker();
    fs::remove_dir_all(root).unwrap();
    assert!(
        exited,
        "cancel must interrupt a FIFO worker after it has consumed initial data"
    );
}

#[test]
fn cancellation_notifies_all_pending_open_waits() {
    for kind in 0..4 {
        let (root, request, _) = fixture();
        fs::remove_file(root.join("playlist.m3u8")).unwrap();
        fs::remove_file(root.join("segment_00000.ts")).unwrap();
        let cache = Arc::new(GrowingCache::new(root.join("playlist.m3u8")));
        let waiting_cache = cache.clone();
        let waiting_root = root.clone();
        let (started, starts) = mpsc::channel();
        let (finished, finishes) = mpsc::channel();
        let waiter = thread::spawn(move || {
            started.send(()).unwrap();
            let result = match kind {
                0 => wait_for_initial_cache_bytes(&request, &waiting_cache, Duration::from_secs(5))
                    .map(|_| ()),
                1 => wait_for_cache_range(
                    &request,
                    &waiting_cache,
                    RangeRequest {
                        start: Some(0),
                        end: Some(3),
                    },
                    Duration::from_secs(5),
                )
                .map(|_| ()),
                2 => wait_for_hls_playlist_snapshot(
                    &request,
                    &waiting_root,
                    &waiting_cache,
                    Duration::from_secs(5),
                )
                .map(|_| ()),
                _ => wait_for_hls_segment(
                    &request,
                    &waiting_cache,
                    &waiting_root.join("segment_00000.ts"),
                    "segment_00000.ts",
                ),
            };
            finished.send(result).unwrap();
        });
        starts.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(
            finishes.recv_timeout(Duration::from_millis(25)).is_err(),
            "open must wait for unavailable input"
        );
        cache.mark_cancelled();
        let result = finishes.recv_timeout(Duration::from_millis(500));
        if result.is_err() {
            cache.mark_error(relay_error(
                "test_release",
                503,
                "Release blocked test waiter.",
                Vec::<(String, String)>::new(),
            ));
        }
        waiter.join().unwrap();
        fs::remove_dir_all(root).unwrap();
        assert!(
            matches!(result, Ok(Err(error)) if error.code == "host_input_cancelled" && error.status == 499),
            "pending open kind {kind} must return typed cancellation promptly"
        );
    }
}

#[cfg(unix)]
#[test]
fn concurrent_reserves_share_one_cache_and_worker_and_retired_lease_cannot_recreate_it() {
    initialize_ffmpeg().unwrap();
    let (root, mut request, _) = fixture();
    let fifo = root.join("track.fifo");
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    request.tracks = vec![PreparedTrack {
        kind: "audio".into(),
        pipe_path: fifo.to_string_lossy().into_owned(),
        media_id: "audio".into(),
        mime_type: Some("audio/mp4".into()),
        codecs: Some("mp4a.40.2".into()),
    }];
    let start = Arc::new(std::sync::Barrier::new(4));
    let callers: Vec<_> = (0..4)
        .map(|_| {
            let mut request = request.clone();
            let start = start.clone();
            thread::spawn(move || {
                start.wait();
                request.session_handle = reserve_session(request.session_id.clone());
                let session = session_cache(&request).unwrap();
                let cache = ensure_mpeg_ts_cache(
                    &request,
                    &session.root_dir,
                    &mut lock_or_recover(&session.state),
                )
                .unwrap();
                let worker_id = lock_or_recover(&cache.worker)
                    .as_ref()
                    .unwrap()
                    .thread()
                    .id();
                (request, session, cache, worker_id)
            })
        })
        .collect();
    let results: Vec<_> = callers
        .into_iter()
        .map(|caller| caller.join().unwrap())
        .collect();
    for result in &results {
        assert!(Arc::ptr_eq(&results[0].1, &result.1));
        assert!(Arc::ptr_eq(&results[0].2, &result.2));
        assert_eq!(results[0].3, result.3);
    }
    let old_root = results[0].1.root_dir.clone();
    for result in &results {
        retire_session(result.0.session_handle);
    }
    assert!(!old_root.exists());
    for (request, session, _, _) in results {
        assert!(
            matches!(session_cache(&request), Err(error) if error.code == "host_input_cancelled")
        );
        assert!(
            matches!(ensure_mpeg_ts_cache(&request, &session.root_dir, &mut lock_or_recover(&session.state)), Err(error) if error.code == "host_input_cancelled")
        );
    }
    let new_handle = reserve_session(request.session_id.clone());
    request.session_handle = new_handle;
    let replacement = session_cache(&request).unwrap();
    assert_ne!(replacement.root_dir, old_root);
    retire_session(new_handle);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn late_native_stream_publication_is_rejected_after_cancellation() {
    let (root, request, cache) = fixture();
    cache.mark_cancelled();
    let result = insert_bound_native_stream(
        NativeStream::Bytes(Cursor::new(b"late bytes".to_vec())),
        cache,
        &request,
    );
    assert!(
        matches!(result, Err(error) if error.code == "host_input_cancelled" && error.status == 499)
    );
    fs::remove_dir_all(root).unwrap();
}
