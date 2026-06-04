use crate::types::{DisplayRect, RenderMode};

pub(crate) fn compute_display_rect(
    surface_width: u32,
    surface_height: u32,
    frame_width: u32,
    frame_height: u32,
    render_mode: RenderMode,
) -> DisplayRect {
    let surface_width = surface_width.max(1);
    let surface_height = surface_height.max(1);
    let frame_width = frame_width.max(1);
    let frame_height = frame_height.max(1);

    match render_mode {
        RenderMode::Stretch => DisplayRect {
            x: 0,
            y: 0,
            width: surface_width,
            height: surface_height,
        },
        RenderMode::Fit | RenderMode::Fill => {
            let scale_x = surface_width as f32 / frame_width as f32;
            let scale_y = surface_height as f32 / frame_height as f32;
            let scale = match render_mode {
                RenderMode::Fit => scale_x.min(scale_y),
                RenderMode::Fill => scale_x.max(scale_y),
                RenderMode::Stretch => unreachable!(),
            };
            let width = ((frame_width as f32 * scale).round() as u32).max(1);
            let height = ((frame_height as f32 * scale).round() as u32).max(1);
            let x = surface_width.saturating_sub(width) / 2;
            let y = surface_height.saturating_sub(height) / 2;

            DisplayRect {
                x,
                y,
                width: width.min(surface_width),
                height: height.min(surface_height),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::compute_display_rect;
    use crate::types::{DisplayRect, RenderMode};

    #[test]
    fn fit_preserves_aspect_ratio_inside_surface() {
        let rect = compute_display_rect(1280, 720, 960, 432, RenderMode::Fit);
        assert_eq!(
            rect,
            DisplayRect {
                x: 0,
                y: 72,
                width: 1280,
                height: 576,
            }
        );
    }

    #[test]
    fn fill_uses_cover_scale_with_current_unsigned_viewport_bounds() {
        let rect = compute_display_rect(1280, 720, 960, 432, RenderMode::Fill);
        assert_eq!(
            rect,
            DisplayRect {
                x: 0,
                y: 0,
                width: 1280,
                height: 720,
            }
        );
    }

    #[test]
    fn stretch_uses_full_surface() {
        let rect = compute_display_rect(1280, 720, 960, 432, RenderMode::Stretch);
        assert_eq!(
            rect,
            DisplayRect {
                x: 0,
                y: 0,
                width: 1280,
                height: 720,
            }
        );
    }

    #[test]
    fn degenerate_dimensions_are_clamped_to_one_pixel() {
        let rect = compute_display_rect(0, 0, 0, 0, RenderMode::Fit);
        assert_eq!(
            rect,
            DisplayRect {
                x: 0,
                y: 0,
                width: 1,
                height: 1,
            }
        );
    }
}
