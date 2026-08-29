use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::PluginTransport;

/// Workload class used to select a plugin invocation transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PluginInvocationWorkload {
    /// Latency-sensitive media decode, processing, or normalization.
    RealtimeMedia,
    /// Bounded observation of runtime events and diagnostics.
    Observer,
    /// Bounded work outside the realtime media path.
    Offline,
}

/// Typed rejection from the host-owned transport workload policy.
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
#[error("plugin transport {transport:?} cannot serve {workload:?} workload")]
pub struct PluginInvocationPolicyError {
    workload: PluginInvocationWorkload,
    transport: PluginTransport,
}

impl PluginInvocationPolicyError {
    pub const fn workload(self) -> PluginInvocationWorkload {
        self.workload
    }

    pub const fn transport(self) -> PluginTransport {
        self.transport
    }
}

/// Host-owned policy that keeps transport labels separate from workload safety.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PluginInvocationPolicy;

impl PluginInvocationPolicy {
    pub const fn standard() -> Self {
        Self
    }

    pub fn validate(
        self,
        workload: PluginInvocationWorkload,
        transport: PluginTransport,
    ) -> Result<(), PluginInvocationPolicyError> {
        if transport == PluginTransport::Wasm && workload == PluginInvocationWorkload::RealtimeMedia
        {
            return Err(PluginInvocationPolicyError {
                workload,
                transport,
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_policy_rejects_only_wasm_realtime_media() {
        let policy = PluginInvocationPolicy::standard();
        for workload in [
            PluginInvocationWorkload::RealtimeMedia,
            PluginInvocationWorkload::Observer,
            PluginInvocationWorkload::Offline,
        ] {
            assert_eq!(policy.validate(workload, PluginTransport::Native), Ok(()));
        }
        assert_eq!(
            policy.validate(
                PluginInvocationWorkload::RealtimeMedia,
                PluginTransport::Wasm,
            ),
            Err(PluginInvocationPolicyError {
                workload: PluginInvocationWorkload::RealtimeMedia,
                transport: PluginTransport::Wasm,
            })
        );
        for workload in [
            PluginInvocationWorkload::Observer,
            PluginInvocationWorkload::Offline,
        ] {
            assert_eq!(policy.validate(workload, PluginTransport::Wasm), Ok(()));
        }
    }
}
