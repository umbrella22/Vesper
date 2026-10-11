use player_dash_hls_bridge::ops::execute_json;
use serde_json::Value;

#[test]
fn android_relay_and_rust_share_template_json_contract() {
    let fixtures: Vec<Value> = serde_json::from_str(include_str!(
        "../../../../lib/android/vesper-player-kit-external-playback/src/test/resources/dash-template-bridge-contract.json"
    ))
    .expect("shared Android relay fixtures");
    for fixture in fixtures {
        let name = fixture["name"].as_str().expect("fixture name");
        let result = execute_json(&fixture["request"].to_string());
        if let Some(error) = fixture["errorContains"].as_str() {
            let actual = result.expect_err(name).to_string();
            assert!(
                actual.contains(error),
                "{name}: expected {error:?}, got {actual:?}"
            );
        } else {
            let response: Value = serde_json::from_str(&result.expect(name)).expect(name);
            assert_eq!(response, fixture["response"], "{name}");
        }
    }
}
