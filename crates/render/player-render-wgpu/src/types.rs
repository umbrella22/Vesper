#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderSurfaceConfig {
    pub width: u32,
    pub height: u32,
}

impl Default for RenderSurfaceConfig {
    fn default() -> Self {
        Self {
            width: 1280,
            height: 720,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RenderMode {
    #[default]
    Fit,
    Fill,
    Stretch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisplayRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone)]
pub struct RgbaVideoFrame {
    pub width: u32,
    pub height: u32,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct Yuv420pVideoFrame {
    pub width: u32,
    pub height: u32,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
pub enum VideoFrameTexture {
    Rgba(RgbaVideoFrame),
    Yuv420p(Yuv420pVideoFrame),
}

#[derive(Debug, Clone)]
pub struct RgbaOverlayFrame {
    pub width: u32,
    pub height: u32,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderFrameOutcome {
    Presented,
    Timeout,
    Occluded,
    SurfaceReconfigured,
}
