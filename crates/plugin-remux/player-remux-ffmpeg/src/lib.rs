#![warn(clippy::undocumented_unsafe_blocks)]

mod error;
mod muxer;

use player_plugin::{Plugin, PluginBuildError};

pub use muxer::FfmpegRemuxProcessor;

#[player_plugin::export]
fn ffmpeg_remux_plugin() -> Result<Plugin, PluginBuildError> {
    Plugin::builder(
        "io.github.ikaros.vesper.remux-ffmpeg",
        "player-remux-ffmpeg",
    )?
    .with_post_download_processor(
        "io.github.ikaros.vesper.remux-ffmpeg.post-download",
        FfmpegRemuxProcessor::new(),
    )?
    .build()
}

#[cfg(test)]
mod tests {
    use super::vesper_plugin_entry;

    #[test]
    fn exports_plugin_entry() {
        let entry: extern "C" fn() -> *const player_plugin::__private::VesperPluginRoot =
            vesper_plugin_entry;
        assert!(!entry().is_null());
    }
}
