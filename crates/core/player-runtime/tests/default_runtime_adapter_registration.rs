use std::sync::atomic::{AtomicUsize, Ordering};

use player_model::MediaSource;
use player_runtime::{
    PlayerError, PlayerErrorCode, PlayerResult, PlayerRuntimeAdapterFactory,
    PlayerRuntimeAdapterInitializer, PlayerRuntimeInitializer, PlayerRuntimeOptions,
    register_default_runtime_adapter_factory,
};

struct ProbeFactory {
    id: &'static str,
    probe_message: &'static str,
    probes: AtomicUsize,
}

impl PlayerRuntimeAdapterFactory for ProbeFactory {
    fn adapter_id(&self) -> &'static str {
        self.id
    }

    fn probe_source_with_options(
        &self,
        _source: MediaSource,
        _options: PlayerRuntimeOptions,
    ) -> PlayerResult<Box<dyn PlayerRuntimeAdapterInitializer>> {
        self.probes.fetch_add(1, Ordering::Relaxed);
        // A distinctive public error identifies the selected factory without media I/O.
        Err(PlayerError::new(
            PlayerErrorCode::Unsupported,
            self.probe_message,
        ))
    }
}

static ORIGINAL_A: ProbeFactory = ProbeFactory {
    id: "registration-test-a",
    probe_message: "probe owned by original A",
    probes: AtomicUsize::new(0),
};
static SAME_ID_OTHER_OBJECT: ProbeFactory = ProbeFactory {
    id: "registration-test-a",
    probe_message: "probe owned by another object with A's id",
    probes: AtomicUsize::new(0),
};
static DIFFERENT_B: ProbeFactory = ProbeFactory {
    id: "registration-test-b",
    probe_message: "probe owned by B",
    probes: AtomicUsize::new(0),
};

// Keep the process-wide OnceLock contract in one integration test binary. Other
// integration and library tests run in separate processes and need no registry reset.
#[test]
fn default_registration_is_idempotent_by_id_and_rejects_replacement() {
    assert!(register_default_runtime_adapter_factory(&ORIGINAL_A).is_ok());
    assert!(register_default_runtime_adapter_factory(&ORIGINAL_A).is_ok());
    assert!(register_default_runtime_adapter_factory(&SAME_ID_OTHER_OBJECT).is_ok());

    let replacement = register_default_runtime_adapter_factory(&DIFFERENT_B);
    eprintln!("register different B after original A: {replacement:?}");

    // Probe after both attempted registrations, even in the failing preimage.
    let probe_error = match PlayerRuntimeInitializer::probe_uri("file:///registry-probe.mp4") {
        Err(error) => error,
        Ok(_) => panic!("the selected test factory must return its identifying probe error"),
    };
    assert_eq!(probe_error.code(), PlayerErrorCode::Unsupported);
    assert_eq!(probe_error.message(), ORIGINAL_A.probe_message);
    assert_eq!(ORIGINAL_A.probes.load(Ordering::Relaxed), 1);
    assert_eq!(SAME_ID_OTHER_OBJECT.probes.load(Ordering::Relaxed), 0);
    assert_eq!(DIFFERENT_B.probes.load(Ordering::Relaxed), 0);
    eprintln!("default public probe still selects the original A object");

    let error = match replacement {
        Err(error) => error,
        Ok(()) => panic!("registering different adapter B after A must return InvalidState"),
    };
    assert_eq!(error.code(), PlayerErrorCode::InvalidState);
    assert_eq!(
        error.message(),
        "default runtime adapter factory is already registered as 'registration-test-a'; cannot replace it with 'registration-test-b'",
    );
}
