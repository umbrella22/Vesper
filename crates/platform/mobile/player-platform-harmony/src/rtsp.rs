use crate::{failure, success};
use serde_json::Value;

#[derive(Default)]
pub struct Rtsp {
    #[cfg(feature = "rtsp-bridge")]
    bridge: Option<player_rtsp_hls_bridge::host::Session>,
}

impl Rtsp {
    pub fn execute(&mut self, input: &Value) -> Value {
        #[cfg(not(feature = "rtsp-bridge"))]
        {
            let _ = (input, success);
            failure(
                "rtsp_bridge_not_bundled",
                "RTSP playback requires a HarmonyOS build with the optional RTSP bridge.",
            )
        }
        #[cfg(feature = "rtsp-bridge")]
        match input.get("op").and_then(Value::as_str) {
            Some("open") if self.bridge.is_none() => {
                let Some(uri) = input.get("uri").and_then(Value::as_str) else {
                    return failure("rtsp_invalid_url", "Expected an RTSP URL.");
                };
                match player_rtsp_hls_bridge::host::Session::start(uri) {
                    Ok(session) => {
                        self.bridge = Some(session);
                        success(Value::Bool(true))
                    }
                    Err(error) => failure(error.code(), error.message()),
                }
            }
            Some("status") => match self.bridge.as_mut() {
                Some(session) => success(session.status()),
                None => failure("rtsp_bridge_closed", "The RTSP bridge session is closed."),
            },
            _ => failure(
                "rtsp_invalid_command",
                "The RTSP bridge command is invalid for this session.",
            ),
        }
    }
}
