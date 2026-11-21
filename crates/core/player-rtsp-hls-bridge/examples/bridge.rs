use player_rtsp_hls_bridge::{Bridge, Phase};
use std::time::{Duration, Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let source = std::env::args()
        .nth(1)
        .ok_or("usage: bridge rtsp://host:port/path")?;
    let bridge = Bridge::start(&source)?;
    let mut last = None;
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut ready = false;
    loop {
        let snapshot = bridge.snapshot();
        if last != Some(snapshot.phase) {
            eprintln!("{:?}", snapshot.phase);
            last = Some(snapshot.phase);
        }
        match snapshot.phase {
            Phase::Ready if !ready => {
                println!("{}", bridge.playlist_url());
                ready = true;
            }
            Phase::Failed(error) => return Err(error.into()),
            _ => (),
        }
        if !ready && Instant::now() > deadline {
            return Err("Timed out waiting for two complete fragments".into());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}
