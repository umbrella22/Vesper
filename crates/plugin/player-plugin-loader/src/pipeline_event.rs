use super::*;

#[derive(Debug)]
pub(crate) struct DynamicPipelineEventHookInner {
    #[allow(dead_code)]
    library: Option<Arc<LibraryHolder>>,
    #[allow(dead_code)]
    name: String,
    api: CheckedPipelineEventHookApi,
}

impl Drop for DynamicPipelineEventHookInner {
    fn drop(&mut self) {
        if let Some(destroy) = self.api.destroy {
            // SAFETY: `destroy` and `context` come from the validated plugin ABI
            // table and are only invoked once when this wrapper is dropped.
            unsafe { destroy(self.api.context) };
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct DynamicPipelineEventHook {
    inner: Arc<DynamicPipelineEventHookInner>,
}

impl DynamicPipelineEventHook {
    pub(crate) fn new(
        library: Option<Arc<LibraryHolder>>,
        fallback_name: String,
        api: CheckedPipelineEventHookApi,
    ) -> Result<Self, PluginLoadError> {
        let name = if let Some(name_fn) = api.name {
            // SAFETY: the plugin ABI declares `name_fn` with `api.context`, and
            // the returned pointer is interpreted immediately as an optional
            // NUL-terminated UTF-8 string.
            let name_ptr = unsafe { name_fn(api.context) };
            if name_ptr.is_null() {
                fallback_name
            } else {
                c_string_field(name_ptr, "hook_name")?
            }
        } else {
            fallback_name
        };

        Ok(Self {
            inner: Arc::new(DynamicPipelineEventHookInner { library, name, api }),
        })
    }
}

impl PipelineEventHook for DynamicPipelineEventHook {
    fn on_event(&self, event: &PipelineEvent) {
        let Ok(event_json) = serde_json::to_vec(event) else {
            return;
        };

        // SAFETY: the validated hook API guarantees `on_event_json` is present,
        // and `event_json` remains alive for the duration of this synchronous
        // callback.
        let _ = unsafe {
            (self.inner.api.on_event_json)(
                self.inner.api.context,
                event_json.as_ptr(),
                event_json.len(),
            )
        };
    }
}
