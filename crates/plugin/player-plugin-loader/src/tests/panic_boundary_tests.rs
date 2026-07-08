use super::*;

#[test]
fn decoder_plugin_panics_are_reported_as_abi_violations() {
    let error = catch_decoder_plugin_call("panic-decoder", "send_packet", || {
        panic!("decoder exploded");
    })
    .expect_err("panic should become a decoder error");

    let message = error.to_string();
    assert!(message.contains("panic-decoder"));
    assert!(message.contains("send_packet"));
    assert!(message.contains("decoder exploded"));
}

#[test]
fn source_normalizer_plugin_panics_are_reported_as_abi_violations() {
    let error = catch_source_normalizer_plugin_call("panic-normalizer", "read_packet", || {
        panic!("normalizer exploded");
    })
    .expect_err("panic should become a source normalizer error");

    let message = error.to_string();
    assert!(message.contains("panic-normalizer"));
    assert!(message.contains("read_packet"));
    assert!(message.contains("normalizer exploded"));
}

#[test]
fn frame_processor_plugin_panics_are_reported_as_abi_violations() {
    let error = catch_frame_processor_plugin_call("panic-processor", "submit_frame", || {
        panic!("processor exploded");
    })
    .expect_err("panic should become a frame processor error");

    let message = error.to_string();
    assert!(message.contains("panic-processor"));
    assert!(message.contains("submit_frame"));
    assert!(message.contains("processor exploded"));
}

#[test]
fn dynamic_library_holder_is_process_lifetime() {
    assert!(
        !std::mem::needs_drop::<super::super::LibraryHolder>(),
        "dynamic plugin libraries must not dlclose while plugin worker threads may still run TLS destructors",
    );
}
