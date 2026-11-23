#![warn(clippy::undocumented_unsafe_blocks)]

#[cfg(target_os = "macos")]
#[allow(
    dead_code,
    reason = "The generated header also contains C++-only types."
)]
mod abi;
#[cfg(target_os = "macos")]
mod audio;
#[cfg(target_os = "macos")]
mod audio_device;
#[cfg(target_os = "macos")]
mod dsp;
#[cfg(target_os = "macos")]
mod dynamic;
#[cfg(target_os = "macos")]
mod metal;
#[cfg(target_os = "macos")]
mod metrics;
#[cfg(target_os = "macos")]
mod packets;
#[cfg(target_os = "macos")]
mod video;

#[cfg(target_os = "macos")]
fn main() -> anyhow::Result<()> {
    let args = std::env::args_os()
        .skip(1)
        .map(std::path::PathBuf::from)
        .collect::<Vec<_>>();
    let [gain, metal, plugins, source, extra @ ..] = args.as_slice() else {
        anyhow::bail!(
            "usage: native_plugin_probe <libgain.dylib> <libmetal_probe.dylib> <plugin-dir> <fixture.mp4> [stereo.f32le]"
        );
    };
    audio::verify(gain)?;
    packets::verify(plugins, source)?;
    video::verify(plugins, metal, source)?;
    match extra {
        [] => Ok(()),
        [pcm] => audio_device::verify(gain, pcm),
        _ => anyhow::bail!("expected at most one PCM fixture"),
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("The native plugin probe requires macOS.");
    std::process::exit(1);
}
