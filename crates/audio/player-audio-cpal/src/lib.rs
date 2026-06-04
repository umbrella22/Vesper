//! CPAL audio sink and playback clock adapter for desktop runtime paths.
//!
//! This crate owns desktop audio device discovery, output stream creation, the
//! `rtrb` single-producer/single-consumer ring, and playback clock accounting.
//! It is an internal desktop adapter rather than a shared native-frame pipeline
//! core; shared playback logic should depend on abstract clocks or observers,
//! not on CPAL directly.

#![deny(unsafe_code)]

mod config;
mod ring;
mod sink;
mod stream;
mod timeline;
mod types;

pub use config::{default_output_config, detect_default_output};
pub use sink::{AudioSink, AudioSinkController};
pub use types::{AudioOutputConfig, AudioOutputDescriptor};
