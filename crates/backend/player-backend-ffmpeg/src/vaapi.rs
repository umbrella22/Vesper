//! Linux VAAPI decoder with separately exported luma and chroma DMA-BUF layers.

use std::ffi::CString;
use std::os::fd::{BorrowedFd, RawFd};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use anyhow::{Context, Result, bail, ensure};
use ffmpeg::ffi::{
    AV_CODEC_HW_CONFIG_METHOD_HW_DEVICE_CTX, AV_HWFRAME_MAP_READ, AVCodecContext,
    AVDRMFrameDescriptor, AVHWDeviceType, AVPixelFormat,
};
use ffmpeg::format::Pixel;
use ffmpeg::frame::Video;
use ffmpeg_next as ffmpeg;
use player_model::MediaSource;

use crate::{FfmpegBackend, MediaProbe, VideoDecodeInfo, VideoDecodeSource, VideoDecoderMode};

const VAAPI: AVPixelFormat = AVPixelFormat::AV_PIX_FMT_VAAPI;
const DRM_PRIME: AVPixelFormat = AVPixelFormat::AV_PIX_FMT_DRM_PRIME;
const NONE: AVPixelFormat = AVPixelFormat::AV_PIX_FMT_NONE;
const VAAPI_DEVICE: AVHWDeviceType = AVHWDeviceType::AV_HWDEVICE_TYPE_VAAPI;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Native420Format {
    Nv12,
    P010,
}

/// One FFmpeg decode stream that tries VAAPI on the selected DRM device.
pub struct VaapiVideoSource {
    decoded: VideoDecodeSource,
    first_frame: Option<VaapiVideoFrame>,
}

impl FfmpegBackend {
    pub fn open_vaapi_video_source(
        &self,
        source: MediaSource,
        interrupt_flag: Option<Arc<AtomicBool>>,
        drm_device: &Path,
    ) -> Result<VaapiVideoSource> {
        let device = CString::new(drm_device.as_os_str().as_bytes())?;
        self.open_video_decode_source(source, interrupt_flag, |parameters| {
            open_decoder(parameters, &device)
        })
        .map(|decoded| VaapiVideoSource {
            decoded,
            first_frame: None,
        })
    }
}

impl VaapiVideoSource {
    pub fn media_probe(&self, source: &MediaSource) -> Result<MediaProbe> {
        self.decoded.media_probe(source)
    }

    pub fn decode_info(&self) -> &VideoDecodeInfo {
        &self.decoded.decode_info
    }

    pub(crate) fn prime(&mut self) -> Result<()> {
        self.first_frame = Some(
            self.next_frame()?
                .context("VAAPI decoder produced no frame")?,
        );
        Ok(())
    }

    pub fn next_frame(&mut self) -> Result<Option<VaapiVideoFrame>> {
        if let Some(frame) = self.first_frame.take() {
            return Ok(Some(frame));
        }
        Ok(self
            .decoded
            .next_sample()?
            .map(|(image, presentation_time)| VaapiVideoFrame {
                image,
                presentation_time,
            }))
    }

    pub fn reset_to(&mut self, position: Duration) -> Result<()> {
        self.first_frame = None;
        self.decoded.reset_to(position)
    }

    pub fn nominal_frame_interval(&self) -> Duration {
        self.decoded.fallback_frame_interval
    }
}

fn open_decoder(
    parameters: &ffmpeg::codec::Parameters,
    device: &CString,
) -> Result<(ffmpeg::decoder::Video, VideoDecodeInfo)> {
    let codec = ffmpeg::decoder::find(parameters.id()).context("no decoder for video stream")?;
    // SAFETY: codec is associated with a trusted, statically registered FFmpeg decoder.
    let supported = unsafe {
        (0..)
            .map(|index| ffmpeg::ffi::avcodec_get_hw_config(codec.as_ptr(), index))
            .take_while(|config| !config.is_null())
            .any(|config| {
                let config = &*config;
                config.device_type == VAAPI_DEVICE
                    && config.pix_fmt == VAAPI
                    && config.methods & AV_CODEC_HW_CONFIG_METHOD_HW_DEVICE_CTX as i32 != 0
            })
    };
    ensure!(supported, "decoder does not support VAAPI device contexts");
    let mut context = ffmpeg::codec::context::Context::from_parameters(parameters.clone())?;
    // SAFETY: context owns hw_device_ctx after creation. Dropping it after any
    // failure releases the device. FFmpeg calls get_format only during decode.
    let result = unsafe {
        let raw = &mut *context.as_mut_ptr();
        raw.get_format = Some(select_vaapi_format);
        // The reference presenter holds up to eight queued frames, one pending
        // presentation, and a bounded set of in-flight GPU leases.
        raw.extra_hw_frames = 12;
        ffmpeg::ffi::av_hwdevice_ctx_create(
            &mut raw.hw_device_ctx,
            VAAPI_DEVICE,
            device.as_ptr(),
            std::ptr::null_mut(),
            0,
        )
    };
    check_ffmpeg(result).context("could not open the VAAPI render node")?;
    let decoder = context.decoder().open_as(codec)?.video()?;
    Ok((
        decoder,
        VideoDecodeInfo {
            selected_mode: VideoDecoderMode::Hardware,
            hardware_available: true,
            hardware_backend: Some("vaapi".to_owned()),
            decoder_name: codec.name().to_owned(),
            fallback_reason: None,
        },
    ))
}

unsafe extern "C" fn select_vaapi_format(
    _context: *mut AVCodecContext,
    formats: *const AVPixelFormat,
) -> AVPixelFormat {
    if formats.is_null() {
        return NONE;
    }
    // SAFETY: FFmpeg provides an array terminated by AV_PIX_FMT_NONE.
    unsafe {
        let mut format = formats;
        loop {
            match *format {
                VAAPI => return VAAPI,
                NONE => return NONE,
                _ => format = format.add(1),
            }
        }
    }
}

pub struct VaapiVideoFrame {
    image: Video,
    pub presentation_time: Duration,
}

impl VaapiVideoFrame {
    pub fn extent(&self) -> (u32, u32) {
        (self.image.width(), self.image.height())
    }

    /// Color description in CICP codes, including unspecified values from FFmpeg.
    pub fn color_description(&self) -> (u8, u8, u8, u8) {
        use ffmpeg::ffi::{
            AVColorPrimaries, AVColorRange, AVColorSpace, AVColorTransferCharacteristic,
        };
        (
            AVColorPrimaries::from(self.image.color_primaries()) as u8,
            AVColorTransferCharacteristic::from(self.image.color_transfer_characteristic()) as u8,
            AVColorSpace::from(self.image.color_space()) as u8,
            AVColorRange::from(self.image.color_range()) as u8,
        )
    }

    pub fn export_drm(&self) -> Result<DrmVideoFrame> {
        ensure!(
            self.image.format() == Pixel::VAAPI,
            "expected a VAAPI frame"
        );
        let mut drm = Video::empty();
        // SAFETY: both frames are live. FFmpeg's VAAPI read map synchronizes the
        // surface and the mapped frame retains the producer and exported fds.
        let result = unsafe {
            (*drm.as_mut_ptr()).format = DRM_PRIME as i32;
            ffmpeg::ffi::av_hwframe_map(
                drm.as_mut_ptr(),
                self.image.as_ptr(),
                AV_HWFRAME_MAP_READ as i32,
            )
        };
        check_ffmpeg(result).context("could not export VAAPI frame as DRM PRIME")?;
        let descriptor = descriptor(&drm)?;
        let format = drm_format(descriptor)?;
        Ok(DrmVideoFrame { drm, format })
    }

    /// Downloads a frame only for the staging fallback.
    pub fn download_420(&self) -> Result<Video420Planes> {
        let mut image = Video::empty();
        // SAFETY: the VAAPI lease owns the source throughout the synchronous transfer.
        let result = unsafe {
            ffmpeg::ffi::av_hwframe_transfer_data(image.as_mut_ptr(), self.image.as_ptr(), 0)
        };
        check_ffmpeg(result).context("could not download a VAAPI frame for staging")?;
        let format = match image.format() {
            Pixel::NV12 => Native420Format::Nv12,
            Pixel::P010LE => Native420Format::P010,
            _ => bail!("VAAPI did not provide a supported biplanar 4:2:0 fallback"),
        };
        Ok(Video420Planes { image, format })
    }
}

pub struct Video420Planes {
    image: Video,
    pub format: Native420Format,
}

impl Video420Planes {
    pub fn planes(&self) -> [(&[u8], usize); 2] {
        [0, 1].map(|index| (self.image.data(index), self.image.stride(index)))
    }
}

pub struct DrmVideoFrame {
    drm: Video,
    pub format: Native420Format,
}

pub struct DrmVideoPlane<'a> {
    pub fd: BorrowedFd<'a>,
    pub object_size: u64,
    pub modifier: u64,
    pub offset: u64,
    pub pitch: u64,
}

impl DrmVideoFrame {
    pub fn planes(&self) -> Result<[DrmVideoPlane<'_>; 2]> {
        let descriptor = descriptor(&self.drm)?;
        Ok([drm_plane(descriptor, 0)?, drm_plane(descriptor, 1)?])
    }
}

fn descriptor(frame: &Video) -> Result<&AVDRMFrameDescriptor> {
    ensure!(
        frame.format() == Pixel::DRM_PRIME,
        "expected a DRM PRIME frame"
    );
    // SAFETY: FFmpeg's DRM PRIME frame owns an aligned AVDRMFrameDescriptor in data[0].
    unsafe {
        (*frame.as_ptr()).data[0]
            .cast::<AVDRMFrameDescriptor>()
            .as_ref()
    }
    .context("DRM PRIME descriptor is missing")
}

fn drm_format(descriptor: &AVDRMFrameDescriptor) -> Result<Native420Format> {
    ensure!(
        descriptor.nb_layers == 2,
        "VAAPI export is not a separate-layer image"
    );
    let [luma, chroma] = [descriptor.layers[0].format, descriptor.layers[1].format];
    match (luma.to_le_bytes(), chroma.to_le_bytes()) {
        ([b'R', b'8', b' ', b' '], [b'G', b'R', b'8', b'8']) => Ok(Native420Format::Nv12),
        ([b'R', b'1', b'6', b' '], [b'G', b'R', b'3', b'2']) => Ok(Native420Format::P010),
        _ => bail!("VAAPI exported unsupported DRM luma and chroma formats"),
    }
}

fn drm_plane(descriptor: &AVDRMFrameDescriptor, index: usize) -> Result<DrmVideoPlane<'_>> {
    let layer = &descriptor.layers[index];
    ensure!(layer.nb_planes == 1, "DRM layer uses additional planes");
    let plane = &layer.planes[0];
    let object_index = usize::try_from(plane.object_index)?;
    let object = descriptor
        .objects
        .get(object_index)
        .filter(|_| plane.object_index < descriptor.nb_objects)
        .context("DRM plane has no backing object")?;
    let fd: RawFd = object.fd;
    ensure!(fd >= 0, "DRM object descriptor is invalid");
    Ok(DrmVideoPlane {
        // SAFETY: the FFmpeg mapped frame keeps this descriptor open for the borrow.
        fd: unsafe { BorrowedFd::borrow_raw(fd) },
        object_size: object.size as u64,
        modifier: object.format_modifier,
        offset: u64::try_from(plane.offset)?,
        pitch: u64::try_from(plane.pitch)?,
    })
}

fn check_ffmpeg(code: i32) -> Result<()> {
    if code < 0 {
        return Err(ffmpeg::Error::from(code).into());
    }
    Ok(())
}
