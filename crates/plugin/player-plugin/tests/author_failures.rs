#![deny(unsafe_code)]

use player_plugin::{
    BenchmarkSinkError, BenchmarkSinkFailure, MAX_PLUGIN_ERROR_MESSAGE_BYTES,
    PipelineEventHookError, PipelineEventHookFailure,
};

#[test]
fn event_hook_failures_preserve_the_established_wire_shape_and_text() {
    let message = "  拒绝 \n";
    for (failure, host_error, code) in [
        (
            PipelineEventHookFailure::InvalidInput(message.to_owned()),
            PipelineEventHookError::InvalidInput(message.to_owned()),
            "invalidInput",
        ),
        (
            PipelineEventHookFailure::Rejected(message.to_owned()),
            PipelineEventHookError::Rejected(message.to_owned()),
            "rejected",
        ),
        (
            PipelineEventHookFailure::Failed(message.to_owned()),
            PipelineEventHookError::Failed(message.to_owned()),
            "failed",
        ),
    ] {
        failure.validate().expect("valid message");
        let expected = serde_json::json!({ "code": code, "message": message });
        assert_eq!(
            serde_json::to_value(&failure).expect("encode failure"),
            expected
        );
        assert_eq!(PipelineEventHookError::from(failure.clone()), host_error);
        assert_eq!(
            serde_json::to_vec(&failure).expect("encode author failure"),
            serde_json::to_vec(&host_error).expect("encode host error"),
        );
        assert_eq!(
            serde_json::from_value::<PipelineEventHookFailure>(expected).expect("decode failure"),
            failure,
        );
    }
}

#[test]
fn event_hook_error_limits_count_utf8_bytes_and_do_not_trim_valid_text() {
    for message in [
        "".to_owned(),
        "x".repeat(MAX_PLUGIN_ERROR_MESSAGE_BYTES + 1),
        "界".repeat(86),
    ] {
        for failure in [
            PipelineEventHookFailure::InvalidInput(message.clone()),
            PipelineEventHookFailure::Rejected(message.clone()),
            PipelineEventHookFailure::Failed(message.clone()),
        ] {
            assert!(failure.validate().is_err());
            assert!(
                PipelineEventHookError::from(failure)
                    .validate_author_failure()
                    .is_err()
            );
        }
    }
    for message in [
        " ".to_owned(),
        "x".repeat(MAX_PLUGIN_ERROR_MESSAGE_BYTES),
        format!("{}x", "界".repeat(85)),
    ] {
        let failure = PipelineEventHookFailure::Failed(message.clone());
        failure.validate().expect("message within byte limit");
        assert_eq!(
            PipelineEventHookError::from(failure),
            PipelineEventHookError::Failed(message)
        );
    }
}

#[test]
fn event_hook_host_only_error_payloads_are_not_author_failures() {
    for error in [
        PipelineEventHookError::PayloadCodec("codec".to_owned()),
        PipelineEventHookError::AbiViolation("abi".to_owned()),
        PipelineEventHookError::ProtocolViolation("protocol".to_owned()),
    ] {
        assert_eq!(
            error.validate_author_failure(),
            Err(PipelineEventHookError::ProtocolViolation(
                "plugin returned a host-owned event-hook error kind".to_owned(),
            )),
        );
        let bytes = serde_json::to_vec(&error).expect("encode forged error");
        assert!(serde_json::from_slice::<PipelineEventHookFailure>(&bytes).is_err());
    }
}

#[test]
fn benchmark_failure_preserves_sink_failed_and_existing_message_semantics() {
    for message in [
        "".to_owned(),
        "  保留 \n".to_owned(),
        "x".repeat(MAX_PLUGIN_ERROR_MESSAGE_BYTES + 1),
    ] {
        let failure = BenchmarkSinkFailure::Failed(message.clone());
        let host_error = BenchmarkSinkError::SinkFailed(message.clone());
        assert_eq!(
            serde_json::to_value(&failure).expect("encode failure"),
            serde_json::json!({ "code": "sinkFailed", "message": message }),
        );
        assert_eq!(BenchmarkSinkError::from(failure.clone()), host_error);
        assert_eq!(
            serde_json::to_vec(&failure).expect("encode author failure"),
            serde_json::to_vec(&host_error).expect("encode host error"),
        );
    }
}

#[test]
fn host_report_projection_preserves_codes_and_utf8_without_mutating_diagnostics() {
    use player_plugin::{MAX_PLUGIN_ERROR_MESSAGE_BYTES, PipelineEventHookError};
    let message = "界".repeat(100);
    let original = PipelineEventHookError::AbiViolation(message.clone());
    let projected = original.report_projection();
    let PipelineEventHookError::AbiViolation(text) = projected else {
        panic!("projection must preserve the host code");
    };
    assert!(text.len() <= MAX_PLUGIN_ERROR_MESSAGE_BYTES);
    assert_eq!(text, "界".repeat(85));
    assert_eq!(original, PipelineEventHookError::AbiViolation(message));
    assert_eq!(
        PipelineEventHookError::ProtocolViolation(String::new()).report_projection(),
        PipelineEventHookError::ProtocolViolation("plugin host error".to_owned())
    );
}
