use anyhow::{Result, anyhow, bail};

// The experimental desktop software path accepts one tightly packed 4096x4096
// RGBA texture, or an equivalent smaller payload, per upload.
pub(crate) const MAX_DESKTOP_SOFTWARE_UPLOAD_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TextureInputLimits {
    pub max_dimension_2d: u32,
    pub max_upload_bytes: u64,
}

impl TextureInputLimits {
    pub(crate) fn for_device(max_dimension_2d: u32, max_buffer_size: u64) -> Self {
        Self {
            max_dimension_2d,
            max_upload_bytes: max_buffer_size.min(MAX_DESKTOP_SOFTWARE_UPLOAD_BYTES),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RgbaTextureLayout {
    pub width: u32,
    pub height: u32,
    pub bytes_per_row: u32,
    pub expected_len: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Yuv420pTextureLayout {
    pub width: u32,
    pub height: u32,
    pub uv_width: u32,
    pub uv_height: u32,
    pub y_plane_len: usize,
    pub uv_plane_len: usize,
    pub expected_len: usize,
}

pub(crate) fn validate_rgba_texture(
    label: &str,
    width: u32,
    height: u32,
    limits: TextureInputLimits,
) -> Result<RgbaTextureLayout> {
    validate_dimensions(label, width, height, limits)?;
    let bytes_per_row = width
        .checked_mul(4)
        .ok_or_else(|| anyhow!("{label} row byte count overflowed"))?;
    let expected_len = checked_plane_len(label, bytes_per_row, height)?;
    validate_byte_budget(label, expected_len, limits)?;
    Ok(RgbaTextureLayout {
        width,
        height,
        bytes_per_row,
        expected_len,
    })
}

pub(crate) fn validate_texture_dimensions(
    label: &str,
    width: u32,
    height: u32,
    max_dimension_2d: u32,
) -> Result<()> {
    validate_dimensions(
        label,
        width,
        height,
        TextureInputLimits {
            max_dimension_2d,
            max_upload_bytes: u64::MAX,
        },
    )
}

pub(crate) fn validate_rgba_upload(
    label: &str,
    width: u32,
    height: u32,
    actual_len: usize,
    limits: TextureInputLimits,
) -> Result<RgbaTextureLayout> {
    let layout = validate_rgba_texture(label, width, height, limits)?;
    validate_payload_len(label, actual_len, layout.expected_len)?;
    Ok(layout)
}

pub(crate) fn validate_yuv420p_upload(
    label: &str,
    width: u32,
    height: u32,
    actual_len: usize,
    limits: TextureInputLimits,
) -> Result<Yuv420pTextureLayout> {
    validate_dimensions(label, width, height, limits)?;
    let uv_width = width / 2 + width % 2;
    let uv_height = height / 2 + height % 2;
    let y_plane_len = checked_plane_len(label, width, height)?;
    let uv_plane_len = checked_plane_len(label, uv_width, uv_height)?;
    let expected_len = uv_plane_len
        .checked_mul(2)
        .and_then(|uv_len| y_plane_len.checked_add(uv_len))
        .ok_or_else(|| anyhow!("{label} total byte count overflowed"))?;
    validate_byte_budget(label, expected_len, limits)?;
    validate_payload_len(label, actual_len, expected_len)?;
    Ok(Yuv420pTextureLayout {
        width,
        height,
        uv_width,
        uv_height,
        y_plane_len,
        uv_plane_len,
        expected_len,
    })
}

fn validate_dimensions(
    label: &str,
    width: u32,
    height: u32,
    limits: TextureInputLimits,
) -> Result<()> {
    if width == 0 || height == 0 {
        bail!("{label} dimensions must be non-zero, got {width}x{height}");
    }
    if width > limits.max_dimension_2d || height > limits.max_dimension_2d {
        bail!(
            "{label} dimensions {width}x{height} exceed the device 2D texture limit {}",
            limits.max_dimension_2d
        );
    }
    Ok(())
}

fn checked_plane_len(label: &str, bytes_per_row: u32, height: u32) -> Result<usize> {
    let bytes_per_row = usize::try_from(bytes_per_row)
        .map_err(|_| anyhow!("{label} row byte count does not fit usize"))?;
    let height =
        usize::try_from(height).map_err(|_| anyhow!("{label} height does not fit usize"))?;
    bytes_per_row
        .checked_mul(height)
        .ok_or_else(|| anyhow!("{label} plane byte count overflowed"))
}

fn validate_byte_budget(
    label: &str,
    expected_len: usize,
    limits: TextureInputLimits,
) -> Result<()> {
    let expected_len = u64::try_from(expected_len)
        .map_err(|_| anyhow!("{label} byte count does not fit the device limit representation"))?;
    if expected_len > limits.max_upload_bytes {
        bail!(
            "{label} requires {expected_len} bytes, exceeding the desktop software texture budget of {} bytes",
            limits.max_upload_bytes
        );
    }
    Ok(())
}

fn validate_payload_len(label: &str, actual_len: usize, expected_len: usize) -> Result<()> {
    if actual_len != expected_len {
        bail!("{label} requires exactly {expected_len} tightly packed bytes, got {actual_len}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        TextureInputLimits, validate_rgba_texture, validate_rgba_upload,
        validate_texture_dimensions, validate_yuv420p_upload,
    };

    const LIMITS: TextureInputLimits = TextureInputLimits {
        max_dimension_2d: 8_192,
        max_upload_bytes: 64 * 1024 * 1024,
    };

    #[test]
    fn rgba_upload_requires_non_zero_exact_tightly_packed_bytes() {
        let layout =
            validate_rgba_upload("RGBA video frame", 2, 2, 16, LIMITS).expect("valid RGBA upload");
        assert_eq!(layout.bytes_per_row, 8);
        assert_eq!(layout.expected_len, 16);

        assert!(validate_rgba_upload("RGBA video frame", 0, 2, 0, LIMITS).is_err());
        assert!(validate_rgba_upload("RGBA video frame", 2, 2, 15, LIMITS).is_err());
        assert!(validate_rgba_upload("RGBA video frame", 2, 2, 17, LIMITS).is_err());
    }

    #[test]
    fn yuv420p_upload_handles_odd_dimensions_and_requires_exact_bytes() {
        let layout = validate_yuv420p_upload("YUV420p video frame", 3, 3, 17, LIMITS)
            .expect("valid odd-sized YUV420p upload");
        assert_eq!((layout.uv_width, layout.uv_height), (2, 2));
        assert_eq!(layout.y_plane_len, 9);
        assert_eq!(layout.uv_plane_len, 4);
        assert_eq!(layout.expected_len, 17);

        assert!(validate_yuv420p_upload("YUV420p video frame", 3, 3, 16, LIMITS).is_err());
        assert!(validate_yuv420p_upload("YUV420p video frame", 3, 3, 18, LIMITS).is_err());
    }

    #[test]
    fn texture_validation_enforces_device_dimension_and_desktop_budget() {
        assert!(validate_rgba_texture("texture", 8_193, 1, LIMITS).is_err());
        assert!(validate_rgba_texture("texture", 4_096, 4_096, LIMITS).is_ok());
        assert!(validate_rgba_texture("texture", 4_097, 4_096, LIMITS).is_err());
        assert!(validate_texture_dimensions("surface", 8_192, 8_192, 8_192).is_ok());
        assert!(validate_texture_dimensions("surface", 8_193, 1, 8_192).is_err());
        assert!(validate_texture_dimensions("YUV frame", 7_680, 4_320, 8_192).is_ok());
        assert!(validate_rgba_texture("RGBA frame", 7_680, 4_320, LIMITS).is_err());
    }

    #[test]
    fn texture_validation_rejects_arithmetic_overflow_before_allocation() {
        let unbounded_dimensions = TextureInputLimits {
            max_dimension_2d: u32::MAX,
            max_upload_bytes: u64::MAX,
        };
        assert!(
            validate_rgba_texture("texture", u32::MAX, u32::MAX, unbounded_dimensions).is_err()
        );
    }
}
