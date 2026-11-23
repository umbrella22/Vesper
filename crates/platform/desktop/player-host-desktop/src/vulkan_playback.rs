use std::path::{Path, PathBuf};
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result, ensure};
use player_backend_ffmpeg::{
    BufferedFramePoll, BufferedVideoSource, DrmVideoFrame, Native420Format, VaapiVideoFrame,
};
use player_model::{DecodedVideoFrame, MediaSource, MediaSourceKind, VideoPixelFormat};
use player_platform_desktop::{
    DesktopVideoFrame, DesktopVideoFramePoll, DesktopVideoFramePresentation, DesktopVideoSource,
    DesktopVideoSourceBootstrap, DesktopVideoSourceFactory, FfmpegDesktopVideoSourceFactory,
};
use player_plugin_abi::{VESPER_PIXEL_NV12, VESPER_PIXEL_P010, VesperColorInfo, VesperVideoFormat};
use player_render_vulkan::{
    DmaBufVideoPlane, ImportedDmaBufVideoImage, VulkanColorFrame, VulkanColorInput,
    VulkanFrameLease, VulkanPresentStatus, VulkanUploadPlane, XlibVulkanPresenter,
    read_shader_words,
};
use player_render_wgpu::RenderFrameOutcome;
use player_runtime::{
    PlayerRuntimeAdapterCapabilities, PlayerRuntimeBootstrap, PlayerRuntimeOptions,
};
use winit::{
    raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle},
    window::Window,
};
use x11_dl::xlib::{Display, Xlib};

struct VulkanX11Window {
    xlib: Xlib,
    display: NonNull<Display>,
    _window: Arc<Window>,
}

// SAFETY: winit initializes Xlib threading before opening its X11 connection.
// This owner retains and closes a separate connection after Vulkan WSI retirement.
unsafe impl Send for VulkanX11Window {}
// SAFETY: Vulkan and this owner's final Drop are the only users of the connection.
unsafe impl Sync for VulkanX11Window {}

impl Drop for VulkanX11Window {
    fn drop(&mut self) {
        // SAFETY: the unique owned connection has no remaining WSI users.
        unsafe { (self.xlib.XCloseDisplay)(self.display.as_ptr()) };
    }
}

type DmaBufLease = Arc<(ImportedDmaBufVideoImage, DrmVideoFrame)>;

struct LinuxFrameLease {
    _dma_buf: Option<DmaBufLease>,
    retired: Arc<AtomicU64>,
}

impl VulkanFrameLease for LinuxFrameLease {
    fn complete(self, accepted: bool) {
        if accepted {
            self.retired.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// Reference presentation for clear VAAPI and software video.
#[derive(Clone)]
pub struct LinuxVulkanPlaybackPresenter {
    state: Arc<Mutex<Option<PresenterState>>>,
    epoch: Arc<AtomicU64>,
    retired: Arc<AtomicU64>,
    drm_device: Option<PathBuf>,
}

struct PresenterState {
    presenter: XlibVulkanPresenter<LinuxFrameLease>,
    chroma: Vec<u8>,
    epoch: u64,
    reported_import: bool,
    reported_fallback: bool,
}

impl std::fmt::Debug for LinuxVulkanPlaybackPresenter {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LinuxVulkanPlaybackPresenter")
            .field("drm_device", &self.drm_device)
            .field("epoch", &self.epoch.load(Ordering::Acquire))
            .finish_non_exhaustive()
    }
}

impl LinuxVulkanPlaybackPresenter {
    /// The caller gives the reference presenter exclusive drawing on this fixed-size window.
    pub fn open(window: Arc<Window>, shader: &Path) -> Result<Self> {
        let shader = read_shader_words(&mut std::fs::File::open(shader)?)?;
        let (RawDisplayHandle::Xlib(display), RawWindowHandle::Xlib(target)) = (
            window.display_handle()?.as_raw(),
            window.window_handle()?.as_raw(),
        ) else {
            anyhow::bail!("the Vulkan reference presenter requires X11 with Xlib handles");
        };
        let toolkit_display = display.display.context("Xlib display handle unavailable")?;
        let xlib = Xlib::open().context("open Xlib for Vulkan presentation")?;
        // SAFETY: the borrowed toolkit display is live. XOpenDisplay uses the same X server
        // while giving WSI its own event queue instead of sharing winit's XCB queue.
        let display = unsafe {
            let name = (xlib.XDisplayString)(toolkit_display.as_ptr().cast());
            NonNull::new((xlib.XOpenDisplay)(name))
        }
        .context("connect Vulkan to the X11 server")?;
        let owner = Arc::new(VulkanX11Window {
            xlib,
            display,
            _window: window,
        });
        // SAFETY: the owner retains both the window and separate Xlib connection through retirement.
        let presenter = unsafe {
            XlibVulkanPresenter::open(display.as_ptr().cast(), target.window, owner, &shader)
        }?;
        let drm_device = if presenter.supports_dma_buf() {
            std::env::var_os("VESPER_LINUX_VAAPI_DRM_DEVICE")
                .map(PathBuf::from)
                .or_else(|| {
                    let default = PathBuf::from("/dev/dri/renderD128");
                    default.exists().then_some(default)
                })
        } else {
            None
        };
        Ok(Self {
            state: Arc::new(Mutex::new(Some(PresenterState {
                presenter,
                chroma: Vec::new(),
                epoch: 0,
                reported_import: false,
                reported_fallback: false,
            }))),
            epoch: Arc::new(AtomicU64::new(1)),
            retired: Arc::default(),
            drm_device,
        })
    }

    pub fn begin_epoch(&self) -> Result<()> {
        self.epoch
            .try_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                current.checked_add(1)
            })
            .map_err(|_| anyhow::anyhow!("Vulkan epoch exhausted"))?;
        Ok(())
    }

    pub fn retired_frames(&self) -> u64 {
        self.retired.load(Ordering::Relaxed)
    }

    pub fn try_open_vaapi_runtime(
        &self,
        source: &str,
        options: PlayerRuntimeOptions,
        interrupt_flag: Arc<AtomicBool>,
    ) -> Result<Option<(PlayerRuntimeBootstrap, PlayerRuntimeAdapterCapabilities)>> {
        let Some(drm_device) = self.drm_device.clone() else {
            return Ok(None);
        };
        let source = super::normalize_desktop_host_source_uri(source.to_owned())?;
        let options = super::desktop_runtime_options_for_source(&source, options);
        // This reference path is limited to local clear files. The normal Linux
        // adapter still owns network playback and software-only configurations.
        let media_source = MediaSource::new(source);
        if media_source.kind() != MediaSourceKind::Local {
            return Ok(None);
        }
        let factory = Arc::new(VaapiVideoFactory {
            presenter: self.clone(),
            drm_device,
        });
        let bootstrap =
            player_platform_linux::open_linux_host_runtime_source_with_video_source_factory(
                media_source,
                options,
                interrupt_flag,
                factory,
            )?;
        let capabilities = bootstrap.runtime.capabilities();
        Ok(Some((bootstrap, capabilities)))
    }

    pub fn present(&self, frame: &DecodedVideoFrame) -> Result<RenderFrameOutcome> {
        self.with_state(|state| state.present_software(frame, &self.retired))
    }

    fn present_vaapi(&self, frame: VaapiVideoFrame) -> Result<RenderFrameOutcome> {
        self.with_state(|state| state.present_vaapi(frame, &self.retired))
    }

    fn with_state(
        &self,
        draw: impl FnOnce(&mut PresenterState) -> Result<RenderFrameOutcome>,
    ) -> Result<RenderFrameOutcome> {
        // Startup can move across threads. Check out exclusive rendering state
        // without holding the mutex across driver calls or frame completion.
        let mut checked_out = PresenterCheckout {
            slot: &self.state,
            state: self
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .take(),
        };
        let Some(state) = checked_out.state.as_mut() else {
            return Ok(RenderFrameOutcome::Timeout);
        };
        let epoch = self.epoch.load(Ordering::Acquire);
        if state.epoch != epoch {
            state.presenter.begin_epoch(epoch)?;
            state.epoch = epoch;
        }
        draw(state)
    }
}

struct PresenterCheckout<'a> {
    slot: &'a Mutex<Option<PresenterState>>,
    state: Option<PresenterState>,
}

impl Drop for PresenterCheckout<'_> {
    fn drop(&mut self) {
        if let Some(state) = self.state.take() {
            *self.slot.lock().unwrap_or_else(|error| error.into_inner()) = Some(state);
        }
    }
}

impl PresenterState {
    fn present_software(
        &mut self,
        frame: &DecodedVideoFrame,
        retired: &Arc<AtomicU64>,
    ) -> Result<RenderFrameOutcome> {
        let input = staged_nv12_input(frame, &mut self.chroma)?;
        let format = VesperVideoFormat {
            pixel_format: VESPER_PIXEL_NV12,
            width: frame.width,
            height: frame.height,
            // Matches the software renderer's SDR Rec.709 interpretation of YUV420P.
            color: VesperColorInfo {
                primaries: 1,
                transfer: 1,
                matrix: 1,
                range: 1,
                bit_depth: 8,
                ..Default::default()
            },
            ..Default::default()
        };
        // SAFETY: the renderer copies validated staging slices into Vulkan-owned memory.
        let status = unsafe {
            self.presenter.present(
                self.epoch,
                LinuxFrameLease {
                    _dma_buf: None,
                    retired: Arc::clone(retired),
                },
                VulkanColorFrame {
                    format,
                    gain: 1.0,
                    input: VulkanColorInput::Staged(input),
                },
            )
        }?;
        Ok(present_outcome(status))
    }

    fn present_vaapi(
        &mut self,
        frame: VaapiVideoFrame,
        retired: &Arc<AtomicU64>,
    ) -> Result<RenderFrameOutcome> {
        let (width, height) = frame.extent();
        let (primaries, transfer, matrix, range) = frame.color_description();
        let video_format = |pixel_format| VesperVideoFormat {
            width,
            height,
            pixel_format: match pixel_format {
                Native420Format::Nv12 => VESPER_PIXEL_NV12,
                Native420Format::P010 => VESPER_PIXEL_P010,
            },
            color: VesperColorInfo {
                primaries,
                transfer,
                matrix,
                range,
                bit_depth: match pixel_format {
                    Native420Format::Nv12 => 8,
                    Native420Format::P010 => 10,
                },
                ..Default::default()
            },
            ..Default::default()
        };
        match self.import_vaapi(&frame) {
            Ok(lease) => {
                let accepted = !self.reported_import;
                // SAFETY: the synchronized DRM map and image are retained by
                // the submitted lease, including across seek and presenter drop.
                let status = unsafe {
                    self.presenter.present(
                        self.epoch,
                        LinuxFrameLease {
                            _dma_buf: Some(Arc::clone(&lease)),
                            retired: Arc::clone(retired),
                        },
                        VulkanColorFrame {
                            format: video_format(lease.1.format),
                            gain: 1.0,
                            input: VulkanColorInput::DmaBuf(&lease.0),
                        },
                    )
                }?;
                if accepted && status.is_some() {
                    tracing::info!("Linux VAAPI DMA-BUF zero-copy presentation active");
                    self.reported_import = true;
                }
                Ok(present_outcome(status))
            }
            Err(error) => {
                if !self.reported_fallback {
                    tracing::info!(%error, "VAAPI DMA-BUF import unavailable, staging decoded planes");
                    self.reported_fallback = true;
                }
                let cpu = frame.download_420()?;
                let input = cpu
                    .planes()
                    .map(|(bytes, row_stride)| VulkanUploadPlane { bytes, row_stride });
                // SAFETY: the renderer copies validated CPU planes into its upload slots.
                let status = unsafe {
                    self.presenter.present(
                        self.epoch,
                        LinuxFrameLease {
                            _dma_buf: None,
                            retired: Arc::clone(retired),
                        },
                        VulkanColorFrame {
                            format: video_format(cpu.format),
                            gain: 1.0,
                            input: VulkanColorInput::Staged(input),
                        },
                    )
                }?;
                Ok(present_outcome(status))
            }
        }
    }

    fn import_vaapi(&self, frame: &VaapiVideoFrame) -> Result<DmaBufLease> {
        let drm = frame.export_drm()?;
        let planes = drm.planes()?.map(|plane| DmaBufVideoPlane {
            fd: plane.fd,
            object_size: plane.object_size,
            modifier: plane.modifier,
            offset: plane.offset,
            pitch: plane.pitch,
        });
        let (width, height) = frame.extent();
        let format = match drm.format {
            Native420Format::Nv12 => VESPER_PIXEL_NV12,
            Native420Format::P010 => VESPER_PIXEL_P010,
        };
        // SAFETY: FFmpeg synchronizes the surface before exporting these planes.
        // The imported frame owner retains the map until GPU retirement.
        let image = unsafe {
            ImportedDmaBufVideoImage::import(
                self.presenter.device_context(),
                width,
                height,
                format,
                planes,
            )
        }?;
        Ok(Arc::new((image, drm)))
    }
}

fn present_outcome(status: Option<VulkanPresentStatus>) -> RenderFrameOutcome {
    match status {
        Some(VulkanPresentStatus::Presented { .. } | VulkanPresentStatus::Suboptimal { .. }) => {
            RenderFrameOutcome::Presented
        }
        Some(VulkanPresentStatus::OutOfDate { .. }) => RenderFrameOutcome::SurfaceReconfigured,
        None => RenderFrameOutcome::Timeout,
    }
}

const VAAPI_PREFETCH_CAPACITY: usize = 8;

#[derive(Debug)]
struct VaapiVideoFactory {
    presenter: LinuxVulkanPlaybackPresenter,
    drm_device: PathBuf,
}

impl DesktopVideoSourceFactory for VaapiVideoFactory {
    fn open_video_source(
        &self,
        source: MediaSource,
        capacity: usize,
        interrupt: Option<Arc<AtomicBool>>,
    ) -> Result<DesktopVideoSourceBootstrap> {
        match BufferedVideoSource::new_vaapi(
            source.clone(),
            capacity.min(VAAPI_PREFETCH_CAPACITY),
            interrupt.clone(),
            self.drm_device.clone(),
        ) {
            Ok(bootstrap) => Ok(DesktopVideoSourceBootstrap {
                source: Box::new(VaapiDesktopVideoSource {
                    source: bootstrap.source,
                    presenter: self.presenter.clone(),
                    interrupt,
                }),
                decode_info: bootstrap.decode_info,
                probe: bootstrap.probe,
            }),
            Err(error) => {
                tracing::info!(%error, "VAAPI decode unavailable, selecting software decode and Vulkan staging");
                let mut bootstrap = FfmpegDesktopVideoSourceFactory
                    .open_video_source(source, capacity, interrupt)?;
                bootstrap.decode_info.fallback_reason = Some(error.to_string());
                Ok(bootstrap)
            }
        }
    }
}

struct VaapiDesktopVideoSource {
    source: BufferedVideoSource<VaapiVideoFrame>,
    presenter: LinuxVulkanPlaybackPresenter,
    interrupt: Option<Arc<AtomicBool>>,
}

struct VaapiPresentation {
    frame: VaapiVideoFrame,
    presenter: LinuxVulkanPlaybackPresenter,
    interrupt: Option<Arc<AtomicBool>>,
}

impl std::fmt::Debug for VaapiPresentation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("VaapiPresentation")
            .field("pts", &self.frame.presentation_time)
            .finish_non_exhaustive()
    }
}

impl DesktopVideoFramePresentation for VaapiPresentation {
    fn present(self: Box<Self>) -> Result<()> {
        if self
            .interrupt
            .as_ref()
            .is_none_or(|flag| !flag.load(Ordering::Acquire))
        {
            self.presenter.present_vaapi(self.frame)?;
        }
        Ok(())
    }
}

impl VaapiDesktopVideoSource {
    fn defer(&self, frame: VaapiVideoFrame) -> DesktopVideoFrame {
        let (width, height) = frame.extent();
        DesktopVideoFrame::native_deferred(
            frame.presentation_time,
            width,
            height,
            Box::new(VaapiPresentation {
                frame,
                presenter: self.presenter.clone(),
                interrupt: self.interrupt.clone(),
            }),
        )
    }
}

impl DesktopVideoSource for VaapiDesktopVideoSource {
    fn recv_frame(&mut self) -> Result<Option<DesktopVideoFrame>> {
        Ok(self.source.recv_frame()?.map(|frame| self.defer(frame)))
    }

    fn try_recv_frame(&mut self) -> Result<DesktopVideoFramePoll> {
        Ok(match self.source.try_recv_frame()? {
            BufferedFramePoll::Ready(frame) => DesktopVideoFramePoll::Ready(self.defer(frame)),
            BufferedFramePoll::Pending => DesktopVideoFramePoll::Pending,
            BufferedFramePoll::EndOfStream => DesktopVideoFramePoll::EndOfStream,
        })
    }

    fn seek_to(&mut self, position: Duration) -> Result<Option<DesktopVideoFrame>> {
        self.presenter.begin_epoch()?;
        Ok(self
            .source
            .seek_to(position)?
            .map(|frame| self.defer(frame)))
    }

    fn buffered_frame_count(&self) -> usize {
        self.source.buffered_frame_count()
    }

    fn set_prefetch_limit(&self, limit: usize) {
        self.source.set_prefetch_limit(limit);
    }

    fn max_prefetch_capacity(&self) -> Option<usize> {
        Some(VAAPI_PREFETCH_CAPACITY)
    }
}

fn staged_nv12_input<'a>(
    frame: &'a DecodedVideoFrame,
    chroma: &'a mut Vec<u8>,
) -> Result<[VulkanUploadPlane<'a>; 2]> {
    ensure!(
        frame.pixel_format == VideoPixelFormat::Yuv420p && frame.bytes_per_row == frame.width,
        "the reference Vulkan route requires packed SDR YUV420P"
    );
    ensure!(frame.width != 0 && frame.height != 0, "empty decoded frame");
    let width = frame.width as usize;
    let height = frame.height as usize;
    let y = width
        .checked_mul(height)
        .context("luma plane is too large")?;
    let uv = width
        .div_ceil(2)
        .checked_mul(height.div_ceil(2))
        .context("chroma plane is too large")?;
    let uv_pair = uv.checked_mul(2).context("chroma planes are too large")?;
    ensure!(
        y.checked_add(uv_pair) == Some(frame.bytes.len()),
        "invalid YUV420P plane lengths"
    );
    let (luma, color) = frame.bytes.split_at(y);
    let (u, v) = color.split_at(uv);
    chroma.resize(uv_pair, 0);
    for ((u, v), sample) in u.iter().zip(v).zip(chroma.chunks_exact_mut(2)) {
        sample.copy_from_slice(&[*u, *v]);
    }
    Ok([
        VulkanUploadPlane {
            bytes: luma,
            row_stride: width,
        },
        VulkanUploadPlane {
            bytes: chroma,
            row_stride: width.div_ceil(2) * 2,
        },
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stages_odd_yuv420p_dimensions_without_truncating_chroma() {
        let mut frame = DecodedVideoFrame {
            presentation_time: std::time::Duration::ZERO,
            width: 3,
            height: 3,
            bytes_per_row: 3,
            pixel_format: VideoPixelFormat::Yuv420p,
            bytes: vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 41, 42, 43, 44, 51, 52, 53, 54],
        };
        let mut chroma = Vec::new();
        let [y, uv] = staged_nv12_input(&frame, &mut chroma).expect("packed YUV420P input");
        assert_eq!(y.bytes, &frame.bytes[..9]);
        assert_eq!(y.row_stride, 3);
        assert_eq!(uv.bytes, &[41, 51, 42, 52, 43, 53, 44, 54]);
        assert_eq!(uv.row_stride, 4);
        frame.bytes.pop();
        assert!(staged_nv12_input(&frame, &mut chroma).is_err());
    }
}
