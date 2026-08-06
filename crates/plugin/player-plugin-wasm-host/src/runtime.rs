use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use thiserror::Error;
use wasmtime::component::Component;
use wasmtime::component::types::{ComponentExtern, ComponentItem};
use wasmtime::{Config, Engine, Store};

use crate::host_state::WasmHostState;
use crate::{
    MAX_WASM_PLUGIN_COMPONENT_BYTES, WASM_PLUGIN_EPOCH_TICK_MILLIS, WASM_PLUGIN_HOSTCALL_FUEL,
    WASM_PLUGIN_INVOCATION_FUEL,
};

const VESPER_STRUCTURED_LOG_IMPORT: &str = "vesper:plugin/host";
const VESPER_PROTOCOL_TYPES_IMPORT: &str = "vesper:plugin/protocol";

#[derive(Debug, Error)]
pub enum WasmPluginRuntimeError {
    #[error("failed to configure the WASM plugin runtime: {0}")]
    Configuration(#[from] wasmtime::Error),
    #[error("failed to start the WASM plugin epoch ticker: {0}")]
    EpochTicker(#[source] std::io::Error),
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum WasmPluginHostError {
    #[error("WASM component exceeds the {limit}-byte host limit")]
    ComponentTooLarge { limit: usize },
    #[error("failed to compile WASM component: {0}")]
    Compilation(String),
    #[error("failed to instantiate WASM component: {0}")]
    Instantiation(String),
    #[error("invalid WASM plugin input: {0}")]
    InvalidInput(String),
    #[error("WASM plugin rejected the call: {0}")]
    Rejected(String),
    #[error("WASM plugin failed the call: {0}")]
    PluginFailed(String),
    #[error("WASM plugin protocol violation: {0}")]
    ProtocolViolation(String),
    #[error("WASM plugin trapped or exceeded its execution budget: {0}")]
    Execution(String),
    #[error("WASM plugin instance is quarantined")]
    Quarantined,
    #[error("WASM plugin host queue failure: {0}")]
    Queue(String),
    #[error("timed out waiting for the WASM plugin host queue: {0}")]
    QueueTimeout(String),
}

#[derive(Clone, Debug)]
pub struct WasmPluginRuntime {
    inner: Arc<WasmPluginRuntimeInner>,
}

#[derive(Debug)]
struct WasmPluginRuntimeInner {
    engine: Engine,
    stop_ticker: Arc<AtomicBool>,
    ticker: Option<JoinHandle<()>>,
}

impl WasmPluginRuntime {
    pub fn new() -> Result<Self, WasmPluginRuntimeError> {
        let mut config = Config::new();
        config.wasm_component_model(true);
        config.consume_fuel(true);
        config.epoch_interruption(true);
        let engine = Engine::new(&config)?;
        let stop_ticker = Arc::new(AtomicBool::new(false));
        let ticker_engine = engine.clone();
        let ticker_stop = Arc::clone(&stop_ticker);
        let ticker = thread::Builder::new()
            .name("vesper-wasm-epoch".to_owned())
            .spawn(move || {
                let tick = Duration::from_millis(WASM_PLUGIN_EPOCH_TICK_MILLIS);
                while !ticker_stop.load(Ordering::Acquire) {
                    thread::sleep(tick);
                    ticker_engine.increment_epoch();
                }
            })
            .map_err(WasmPluginRuntimeError::EpochTicker)?;
        Ok(Self {
            inner: Arc::new(WasmPluginRuntimeInner {
                engine,
                stop_ticker,
                ticker: Some(ticker),
            }),
        })
    }

    pub(crate) fn engine(&self) -> &Engine {
        &self.inner.engine
    }

    pub(crate) fn compile_component(&self, bytes: &[u8]) -> Result<Component, WasmPluginHostError> {
        if bytes.len() > MAX_WASM_PLUGIN_COMPONENT_BYTES {
            return Err(WasmPluginHostError::ComponentTooLarge {
                limit: MAX_WASM_PLUGIN_COMPONENT_BYTES,
            });
        }
        let component = Component::from_binary(self.engine(), bytes)
            .map_err(|error| WasmPluginHostError::Compilation(error.to_string()))?;
        let component_type = component.component_type();
        let unexpected_imports = component_type
            .imports(self.engine())
            .filter(|(name, import)| !is_allowed_component_import(self.engine(), name, import))
            .map(|(name, _)| name)
            .collect::<Vec<_>>();
        if !unexpected_imports.is_empty() {
            return Err(WasmPluginHostError::ProtocolViolation(format!(
                "component imports are limited to structured log calls through '{VESPER_STRUCTURED_LOG_IMPORT}' and type-only protocol imports through '{VESPER_PROTOCOL_TYPES_IMPORT}'; found {}",
                unexpected_imports.join(", ")
            )));
        }
        Ok(component)
    }

    pub(crate) fn new_store(
        &self,
        timeout: Duration,
    ) -> Result<Store<WasmHostState>, WasmPluginHostError> {
        let mut store = Store::new(self.engine(), WasmHostState::new());
        store.limiter(|state| &mut state.limits);
        self.prepare_store(&mut store, timeout)?;
        Ok(store)
    }

    pub(crate) fn prepare_store(
        &self,
        store: &mut Store<WasmHostState>,
        timeout: Duration,
    ) -> Result<(), WasmPluginHostError> {
        store.data_mut().begin_call();
        store.set_hostcall_fuel(WASM_PLUGIN_HOSTCALL_FUEL);
        store
            .set_fuel(WASM_PLUGIN_INVOCATION_FUEL)
            .map_err(|error| WasmPluginHostError::Execution(error.to_string()))?;
        store.epoch_deadline_trap();
        store.set_epoch_deadline(epoch_ticks(timeout));
        Ok(())
    }
}

fn is_allowed_component_import(engine: &Engine, name: &str, import: &ComponentExtern<'_>) -> bool {
    if name == VESPER_STRUCTURED_LOG_IMPORT {
        return true;
    }
    if name != VESPER_PROTOCOL_TYPES_IMPORT {
        return false;
    }
    let ComponentItem::ComponentInstance(instance) = &import.ty else {
        return false;
    };
    instance
        .exports(engine)
        .all(|(_, export)| matches!(export.ty, ComponentItem::Type(_)))
}

impl Drop for WasmPluginRuntimeInner {
    fn drop(&mut self) {
        self.stop_ticker.store(true, Ordering::Release);
        if let Some(ticker) = self.ticker.take() {
            let _ = ticker.join();
        }
    }
}

fn epoch_ticks(timeout: Duration) -> u64 {
    let tick_nanos = u128::from(WASM_PLUGIN_EPOCH_TICK_MILLIS) * 1_000_000;
    let timeout_nanos = timeout.as_nanos();
    let ticks = timeout_nanos.saturating_add(tick_nanos - 1) / tick_nanos;
    u64::try_from(ticks.max(1)).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use wasmtime::component::{Component, Linker};

    use super::WasmPluginRuntime;
    use crate::WASM_PLUGIN_TABLE_ELEMENT_LIMIT;

    #[test]
    fn store_rejects_a_component_over_the_memory_budget() {
        let runtime = WasmPluginRuntime::new().expect("WASM runtime");
        let bytes = wat::parse_str(
            r#"
                (component
                    (core module $module
                        (memory 1025))
                    (core instance $instance (instantiate $module)))
            "#,
        )
        .expect("oversized-memory component");
        let component = Component::from_binary(runtime.engine(), &bytes).expect("component");
        let linker = Linker::new(runtime.engine());
        let mut store = runtime
            .new_store(Duration::from_millis(50))
            .expect("bounded store");

        assert!(linker.instantiate(&mut store, &component).is_err());
    }

    #[test]
    fn store_rejects_a_component_over_the_initial_table_budget() {
        let runtime = WasmPluginRuntime::new().expect("WASM runtime");
        let bytes = wat::parse_str(format!(
            r#"
                (component
                    (core module $module
                        (table {} funcref))
                    (core instance $instance (instantiate $module)))
            "#,
            WASM_PLUGIN_TABLE_ELEMENT_LIMIT + 1
        ))
        .expect("oversized-table component");
        let component = Component::from_binary(runtime.engine(), &bytes).expect("component");
        let linker = Linker::new(runtime.engine());
        let mut store = runtime
            .new_store(Duration::from_millis(50))
            .expect("bounded store");

        assert!(linker.instantiate(&mut store, &component).is_err());
    }

    #[test]
    fn store_traps_when_a_table_grows_over_budget() {
        let runtime = WasmPluginRuntime::new().expect("WASM runtime");
        let bytes = wat::parse_str(format!(
            r#"
                (component
                    (core module $module
                        (table 1 funcref)
                        (func (export "grow") (result i32)
                            (table.grow (ref.null func) (i32.const {}))))
                    (core instance $instance (instantiate $module))
                    (func (export "grow") (result s32)
                        (canon lift (core func $instance "grow"))))
            "#,
            WASM_PLUGIN_TABLE_ELEMENT_LIMIT
        ))
        .expect("table-growth component");
        let component = Component::from_binary(runtime.engine(), &bytes).expect("component");
        let linker = Linker::new(runtime.engine());
        let mut store = runtime
            .new_store(Duration::from_millis(50))
            .expect("bounded store");
        let instance = linker
            .instantiate(&mut store, &component)
            .expect("component instance");
        let grow = instance
            .get_typed_func::<(), (i32,)>(&mut store, "grow")
            .expect("typed table growth export");

        grow.call(&mut store, ())
            .expect_err("table growth beyond the host limit must trap");
    }

    #[test]
    fn linker_rejects_unconfigured_wasi_imports() {
        let runtime = WasmPluginRuntime::new().expect("WASM runtime");
        let bytes = wat::parse_str(
            r#"
                (component
                    (type $function-type (func))
                    (import "wasi:cli/environment@0.2.0" (func $environment (type $function-type))))
            "#,
        )
        .expect("component with a WASI import");
        let component = Component::from_binary(runtime.engine(), &bytes).expect("component");
        let linker = Linker::new(runtime.engine());
        let mut store = runtime
            .new_store(Duration::from_millis(50))
            .expect("bounded store");

        let error = linker
            .instantiate(&mut store, &component)
            .expect_err("WASI must not be linked");
        assert!(error.to_string().contains("wasi:cli/environment"));
    }

    #[test]
    fn compile_rejects_wasi_imports_as_protocol_violations() {
        let runtime = WasmPluginRuntime::new().expect("WASM runtime");
        let bytes = wat::parse_str(
            r#"
                (component
                    (type $function-type (func))
                    (import "wasi:io/poll@0.2.9" (func (type $function-type))))
            "#,
        )
        .expect("component with a WASI import");

        let error = match runtime.compile_component(&bytes) {
            Ok(_) => panic!("WASI must not be part of the guest contract"),
            Err(error) => error,
        };
        assert!(matches!(
            error,
            super::WasmPluginHostError::ProtocolViolation(_)
        ));
        assert!(error.to_string().contains("wasi:io/poll@0.2.9"));
    }

    #[test]
    fn compile_allows_the_type_only_protocol_interface_import() {
        let runtime = WasmPluginRuntime::new().expect("WASM runtime");
        let bytes = wat::parse_str(
            r#"
                (component
                    (type $protocol-types (instance))
                    (import "vesper:plugin/protocol"
                        (instance (type $protocol-types))))
            "#,
        )
        .expect("component with the protocol type import");

        runtime
            .compile_component(&bytes)
            .expect("type-only protocol imports do not grant host capabilities");
    }

    #[test]
    fn invocation_fuel_interrupts_an_infinite_guest_loop() {
        let runtime = WasmPluginRuntime::new().expect("WASM runtime");
        let (mut store, instance) = spinning_component(&runtime);
        let spin = instance
            .get_typed_func::<(), ()>(&mut store, "spin")
            .expect("typed spin export");

        let error = spin
            .call(&mut store, ())
            .expect_err("fuel must interrupt the guest");
        assert_eq!(
            error.downcast_ref::<wasmtime::Trap>(),
            Some(&wasmtime::Trap::OutOfFuel)
        );
    }

    #[test]
    fn epoch_deadline_interrupts_a_guest_when_fuel_is_not_exhausted() {
        let runtime = WasmPluginRuntime::new().expect("WASM runtime");
        let (mut store, instance) = spinning_component(&runtime);
        store.set_fuel(u64::MAX).expect("unbounded test fuel");
        store.set_epoch_deadline(2);
        let spin = instance
            .get_typed_func::<(), ()>(&mut store, "spin")
            .expect("typed spin export");
        let started = Instant::now();

        let error = spin
            .call(&mut store, ())
            .expect_err("epoch must interrupt the guest");
        assert_eq!(
            error.downcast_ref::<wasmtime::Trap>(),
            Some(&wasmtime::Trap::Interrupt)
        );
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    fn spinning_component(
        runtime: &WasmPluginRuntime,
    ) -> (
        wasmtime::Store<crate::host_state::WasmHostState>,
        wasmtime::component::Instance,
    ) {
        let bytes = wat::parse_str(
            r#"
                (component
                    (core module $module
                        (func (export "spin")
                            (loop $again
                                br $again)))
                    (core instance $instance (instantiate $module))
                    (func (export "spin")
                        (canon lift (core func $instance "spin"))))
            "#,
        )
        .expect("spinning component");
        let component = Component::from_binary(runtime.engine(), &bytes).expect("component");
        let linker = Linker::new(runtime.engine());
        let mut store = runtime
            .new_store(Duration::from_millis(50))
            .expect("bounded store");
        let instance = linker
            .instantiate(&mut store, &component)
            .expect("component instance");
        (store, instance)
    }
}
