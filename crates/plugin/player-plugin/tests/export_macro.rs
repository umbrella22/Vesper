#![deny(unsafe_code)]

use player_plugin::author::{Plugin, VesperPluginRoot};

#[player_plugin::export]
fn fixture_plugin() -> Plugin {
    Plugin::new("dev.vesper.macro-fixture", "Macro Fixture")
}

#[test]
fn export_macro_generates_the_native_entry_without_author_unsafe() {
    let entry: extern "C" fn() -> *const VesperPluginRoot = vesper_native_plugin_entry;
    assert_ne!(entry as usize, 0);
}
