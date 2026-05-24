use super::*;

pub struct BenchmarkSinkPluginSession {
    sinks: Vec<Arc<dyn BenchmarkSink>>,
}

impl std::fmt::Debug for BenchmarkSinkPluginSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BenchmarkSinkPluginSession")
            .field("sink_count", &self.sinks.len())
            .finish()
    }
}

impl BenchmarkSinkPluginSession {
    pub fn load_paths(
        paths: impl IntoIterator<Item = impl AsRef<Path>>,
    ) -> Result<Self, PluginLoadError> {
        let mut sinks = Vec::new();
        for path in paths {
            let plugin = LoadedDynamicPlugin::load(path.as_ref())?;
            if let Some(sink) = plugin.benchmark_sink() {
                sinks.push(sink);
            }
        }

        Ok(Self { sinks })
    }

    pub fn is_empty(&self) -> bool {
        self.sinks.is_empty()
    }

    pub fn on_event_batch_json(
        &self,
        batch_json: &str,
    ) -> Result<BenchmarkSinkReport, BenchmarkSinkError> {
        let batch = serde_json::from_str::<BenchmarkEventBatch>(batch_json).map_err(|error| {
            BenchmarkSinkError::PayloadCodec(format!(
                "decode benchmark event batch payload failed: {error}"
            ))
        })?;
        Ok(self.on_event_batch(&batch))
    }

    pub fn on_event_batch_report_json(
        &self,
        batch_json: &str,
    ) -> Result<String, BenchmarkSinkError> {
        serde_json::to_string(&self.on_event_batch_json(batch_json)?).map_err(|error| {
            BenchmarkSinkError::PayloadCodec(format!(
                "encode benchmark sink status failed: {error}"
            ))
        })
    }

    pub fn on_event_batch(&self, batch: &BenchmarkEventBatch) -> BenchmarkSinkReport {
        let mut report = BenchmarkSinkReport::default();
        for sink in &self.sinks {
            match sink.on_event_batch(batch) {
                Ok(status) => {
                    report.accepted_events += status.accepted_events;
                }
                Err(error) => {
                    report.dropped_events += batch.events.len() as u64;
                    report
                        .plugin_errors
                        .push(format!("{}: {error}", sink.name()));
                }
            }
        }
        report
    }

    pub fn flush(&self) -> BenchmarkSinkReport {
        let mut report = BenchmarkSinkReport::default();
        for sink in &self.sinks {
            match sink.flush() {
                Ok(sink_report) => {
                    report.accepted_events += sink_report.accepted_events;
                    report.dropped_events += sink_report.dropped_events;
                    report.plugin_errors.extend(sink_report.plugin_errors);
                }
                Err(error) => {
                    report
                        .plugin_errors
                        .push(format!("{}: {error}", sink.name()));
                }
            }
        }
        report
    }

    pub fn flush_json(&self) -> Result<String, BenchmarkSinkError> {
        serde_json::to_string(&self.flush()).map_err(|error| {
            BenchmarkSinkError::PayloadCodec(format!(
                "encode benchmark sink report failed: {error}"
            ))
        })
    }
}

#[derive(Debug)]
pub(crate) struct DynamicBenchmarkSinkInner {
    #[allow(dead_code)]
    library: Option<Arc<LibraryHolder>>,
    name: String,
    api: CheckedBenchmarkSinkApi,
}

impl Drop for DynamicBenchmarkSinkInner {
    fn drop(&mut self) {
        if let Some(destroy) = self.api.destroy {
            // SAFETY: `destroy` and `context` come from the validated plugin ABI
            // table and are only invoked once when this wrapper is dropped.
            unsafe { destroy(self.api.context) };
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct DynamicBenchmarkSink {
    inner: Arc<DynamicBenchmarkSinkInner>,
}

impl DynamicBenchmarkSink {
    pub(crate) fn new(
        library: Option<Arc<LibraryHolder>>,
        fallback_name: String,
        api: CheckedBenchmarkSinkApi,
    ) -> Result<Self, PluginLoadError> {
        let name = if let Some(name_fn) = api.name {
            // SAFETY: the plugin ABI declares `name_fn` with `api.context`, and
            // the returned pointer is interpreted immediately as an optional
            // NUL-terminated UTF-8 string.
            let name_ptr = unsafe { name_fn(api.context) };
            if name_ptr.is_null() {
                fallback_name
            } else {
                c_string_field(name_ptr, "benchmark_sink_name")?
            }
        } else {
            fallback_name
        };

        Ok(Self {
            inner: Arc::new(DynamicBenchmarkSinkInner { library, name, api }),
        })
    }

    fn decode_result<T: DeserializeOwned>(
        &self,
        result: VesperPluginProcessResult,
        operation: &'static str,
    ) -> Result<T, BenchmarkSinkError> {
        match result.status {
            VesperPluginResultStatus::Success => decode_plugin_bytes::<T>(
                result.payload,
                self.inner.api.free_bytes,
                self.inner.api.context,
            )
            .map_err(|error| {
                BenchmarkSinkError::PayloadCodec(format!(
                    "decode benchmark sink `{}` {operation} payload failed: {error}",
                    self.inner.name
                ))
            }),
            VesperPluginResultStatus::Failure => {
                let decoded = decode_plugin_bytes::<BenchmarkSinkError>(
                    result.payload,
                    self.inner.api.free_bytes,
                    self.inner.api.context,
                )
                .unwrap_or_else(|error| {
                    BenchmarkSinkError::PayloadCodec(format!(
                        "decode benchmark sink `{}` {operation} error payload failed: {error}",
                        self.inner.name
                    ))
                });
                Err(decoded)
            }
        }
    }
}

impl BenchmarkSink for DynamicBenchmarkSink {
    fn name(&self) -> &str {
        &self.inner.name
    }

    fn on_event_batch(
        &self,
        batch: &BenchmarkEventBatch,
    ) -> Result<BenchmarkSinkStatus, BenchmarkSinkError> {
        let batch_json = serde_json::to_vec(batch).map_err(|error| {
            BenchmarkSinkError::PayloadCodec(format!(
                "serialize benchmark batch for `{}` failed: {error}",
                self.inner.name
            ))
        })?;

        // SAFETY: the validated sink API guarantees `on_event_batch_json` is
        // present, and `batch_json` remains alive for the duration of this
        // synchronous callback.
        let result = unsafe {
            (self.inner.api.on_event_batch_json)(
                self.inner.api.context,
                batch_json.as_ptr(),
                batch_json.len(),
            )
        };
        self.decode_result(result, "batch")
    }

    fn flush(&self) -> Result<BenchmarkSinkReport, BenchmarkSinkError> {
        let Some(flush_json) = self.inner.api.flush_json else {
            return Ok(BenchmarkSinkReport::default());
        };
        // SAFETY: the validated sink API declares `flush_json` with this
        // context. The callback is synchronous and returns plugin-owned bytes.
        let result = unsafe { flush_json(self.inner.api.context) };
        self.decode_result(result, "flush")
    }
}
