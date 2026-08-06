#![deny(unsafe_code)]

use player_plugin::{
    PipelineEvent, PipelineEventHook, PipelineEventHookError, PipelineEventHookOutcome, Plugin,
    PluginBuildError,
};

struct Hook;

impl PipelineEventHook for Hook {
    fn on_event(
        &self,
        _event: &PipelineEvent,
    ) -> Result<PipelineEventHookOutcome, PipelineEventHookError> {
        Ok(PipelineEventHookOutcome::accepted())
    }
}

#[player_plugin::export]
fn fixture_plugin() -> Result<Plugin, PluginBuildError> {
    Plugin::builder("dev.vesper.macro-fixture", "Macro Fixture")?
        .with_pipeline_event_hook("dev.vesper.macro-fixture.hook", Hook)?
        .build()
}

#[test]
fn export_macro_generates_the_fixed_native_entry_without_author_unsafe() {
    let entry: extern "C" fn() -> *const player_plugin::__private::VesperPluginRoot =
        vesper_plugin_entry;
    assert_ne!(entry as usize, 0);
}
