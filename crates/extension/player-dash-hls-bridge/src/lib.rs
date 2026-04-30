//! Shared DASH-to-HLS bridge primitives.
//!
//! This crate owns platform-neutral DASH MPD parsing, SIDX/MP4 byte-range handling,
//! SegmentTemplate expansion, and HLS playlist generation. Platform host kits should
//! keep their transport, resource-loader, cache, and loopback server code outside this
//! crate, then call these pure operations directly or through the JSON FFI operation
//! layer in [`ops`].

pub mod dash;
pub mod error;
pub mod hls;
pub mod mp4;
pub mod ops;

pub use error::{DashHlsError, DashHlsResult};
