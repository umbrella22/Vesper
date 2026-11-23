//! Shared ABI validation for the export shim and the native loader.

use std::mem::{align_of, size_of};

use super::author::{PluginError, PluginResult};
use super::*;

pub const MAX_JSON_BYTES: usize = 16 * 1024 * 1024;

pub fn check_status(status: VesperStatus) -> PluginResult<()> {
    match status {
        VESPER_OK => Ok(()),
        value => Err(PluginError::Status(value)),
    }
}

pub fn sized_pointer<T>(ptr: *const T) -> bool {
    !ptr.is_null() && ptr.is_aligned()
}

/// # Safety
/// `ptr` is null or addresses a readable ABI header and its advertised prefix.
/// `T` is an ABI POD struct with a leading `u32 struct_size`.
pub unsafe fn read_sized<T: Copy>(ptr: *const T) -> PluginResult<T> {
    // SAFETY: guaranteed by the caller. The check limits the readable prefix.
    unsafe { require_size(ptr, size_of::<T>()) }?;
    // SAFETY: require_size checked alignment and at least size_of::<T>() bytes.
    Ok(unsafe { ptr.read() })
}

/// # Safety
/// `ptr` is null or addresses a readable leading `u32 struct_size`.
pub unsafe fn require_size<T>(ptr: *const T, minimum: usize) -> PluginResult<()> {
    if !sized_pointer(ptr) || !ptr.cast::<u32>().is_aligned() {
        return Err(PluginError::Contract("null or unaligned ABI struct"));
    }
    // SAFETY: the prefix is readable under the caller's contract and aligned.
    if (unsafe { ptr.cast::<u32>().read() } as usize) < minimum {
        return Err(PluginError::Contract("undersized ABI struct"));
    }
    Ok(())
}

pub fn bytes(value: &[u8]) -> VesperBytes {
    VesperBytes {
        data: value.as_ptr(),
        len: value.len() as u64,
    }
}

/// # Safety
/// The caller guarantees `value` is live, immutable and readable for `'a`.
pub unsafe fn read_bytes<'a>(value: VesperBytes, limit: usize) -> PluginResult<&'a [u8]> {
    let len = usize::try_from(value.len).map_err(|_| PluginError::Status(VESPER_ERR_EXHAUSTED))?;
    if len == 0 {
        return Ok(&[]);
    }
    if len > limit.min(isize::MAX as usize) || value.data.is_null() {
        return Err(PluginError::Contract("invalid ABI slice length or pointer"));
    }
    // SAFETY: caller supplies valid, live storage. The length is now bounded.
    Ok(unsafe { std::slice::from_raw_parts(value.data, len) })
}

pub fn validate_f32_audio_format(format: &VesperAudioFormat) -> PluginResult<std::num::NonZeroU16> {
    if (format.struct_size as usize) < size_of::<VesperAudioFormat>()
        || format.format_id == 0
        || format.sample_rate == 0
        || format.max_block_frames == 0
    {
        return Err(PluginError::Contract("invalid audio format"));
    }
    if format.sample_format != VESPER_SAMPLE_F32_INTERLEAVED {
        return Err(PluginError::Status(VESPER_ERR_UNSUPPORTED));
    }
    std::num::NonZeroU16::new(format.channels).ok_or(PluginError::Contract("zero audio channels"))
}

pub fn audio_sample_count(
    buffer: &VesperAudioBuffer,
    format: &VesperAudioFormat,
    use_capacity: bool,
) -> PluginResult<usize> {
    let channels = validate_f32_audio_format(format)?;
    if buffer.format_id != format.format_id
        || buffer.frame_count > buffer.frame_capacity
        || buffer.frame_capacity > u32::from(format.max_block_frames)
    {
        return Err(PluginError::Contract(
            "audio buffer does not match the negotiated format",
        ));
    }
    let frames = if use_capacity {
        buffer.frame_capacity
    } else {
        buffer.frame_count
    };
    let count = (frames as usize)
        .checked_mul(usize::from(channels.get()))
        .filter(|count| *count <= isize::MAX as usize / size_of::<f32>())
        .ok_or(PluginError::Status(VESPER_ERR_EXHAUSTED))?;
    if count != 0 && !sized_pointer(buffer.planes[0].cast::<f32>()) {
        return Err(PluginError::Contract("null or unaligned PCM plane"));
    }
    Ok(count)
}

pub fn pcm_ranges_are_disjoint(
    a: *const c_void,
    a_samples: usize,
    b: *const c_void,
    b_samples: usize,
) -> bool {
    let end = |start: usize, count: usize| {
        count
            .checked_mul(size_of::<f32>())
            .and_then(|len| start.checked_add(len))
    };
    a_samples == 0
        || b_samples == 0
        || matches!((end(a.addr(), a_samples), end(b.addr(), b_samples)), (Some(a_end), Some(b_end))
            if a_end <= b.addr() || b_end <= a.addr())
}

pub fn valid_plugin_id(id: &str) -> bool {
    id.len() <= 255
        && id.contains('.')
        && id.split('.').all(|label| {
            !label.is_empty()
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
}

pub fn alignment_and_minimum_size<T>(ptr: *const T) -> bool {
    sized_pointer(ptr) && ptr.addr().is_multiple_of(align_of::<u32>())
}
