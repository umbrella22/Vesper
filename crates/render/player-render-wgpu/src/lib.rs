//! WGPU software-upload presenter for desktop runtime experiments and examples.
//!
//! This crate owns desktop window/surface rendering, RGB/YUV texture uploads,
//! viewport calculation, and overlay composition for the basic-player path. It
//! is an internal desktop adapter rather than a shared native-frame pipeline
//! core: decoder, processor, frame lease, and platform-native presenter
//! lifecycle remain outside this crate.

#![deny(unsafe_code)]

mod pipeline;
mod renderer;
mod shaders;
mod types;
mod upload;
mod viewport;
mod window;

pub use renderer::VideoRenderer;
pub use types::{
    DisplayRect, RenderFrameOutcome, RenderMode, RenderSurfaceConfig, RgbaOverlayFrame,
    RgbaVideoFrame, VideoFrameTexture, Yuv420pVideoFrame,
};
pub use window::{default_window_attributes, preferred_backends};
