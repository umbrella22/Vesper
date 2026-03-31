#[cfg(not(target_os = "macos"))]
use player_render_wgpu::RgbaOverlayFrame;
#[cfg(not(target_os = "macos"))]
use player_runtime::PlayerSnapshot;

pub use crate::desktop_ui::{CONTROL_RATES, ControlAction, SeekPreview};
#[cfg(not(target_os = "macos"))]
use crate::desktop_ui::{
    DesktopUiLayoutMetrics, DesktopUiRect, DesktopUiViewModel, is_scrubbable_timeline,
};

#[cfg(not(target_os = "macos"))]
#[derive(Debug, Clone, Copy)]
enum ControlVisual {
    SeekStart,
    SeekBack,
    PlayPause,
    Stop,
    SeekForward,
    SeekEnd,
    Rate(&'static str),
}

#[cfg(not(target_os = "macos"))]
#[derive(Debug, Clone, Copy)]
struct ControlButton {
    rect: DesktopUiRect,
    action: ControlAction,
    visual: ControlVisual,
}

#[cfg(not(target_os = "macos"))]
#[derive(Debug, Clone)]
struct ControlLayout {
    metrics: DesktopUiLayoutMetrics,
    bar_rect: DesktopUiRect,
    progress_rect: DesktopUiRect,
    progress_hit_rect: DesktopUiRect,
    buttons: Vec<ControlButton>,
}

#[cfg(not(target_os = "macos"))]
pub fn render_control_overlay(
    frame_width: u32,
    frame_height: u32,
    snapshot: &PlayerSnapshot,
    seek_preview: Option<SeekPreview>,
) -> Option<RgbaOverlayFrame> {
    if frame_width == 0 || frame_height == 0 {
        return None;
    }

    let layout = control_layout(frame_width, frame_height)?;
    let view_model = DesktopUiViewModel::from_snapshot(snapshot, true, seek_preview);
    let mut overlay_bytes = vec![0; frame_width as usize * frame_height as usize * 4];
    draw_control_bar(
        &mut overlay_bytes,
        frame_width,
        frame_height,
        &view_model,
        &layout,
    );

    Some(RgbaOverlayFrame {
        width: frame_width,
        height: frame_height,
        bytes: overlay_bytes,
    })
}

#[cfg(not(target_os = "macos"))]
pub fn control_action_at(
    frame_width: u32,
    frame_height: u32,
    cursor_x: f64,
    cursor_y: f64,
    snapshot: &PlayerSnapshot,
) -> Option<ControlAction> {
    if frame_width == 0 || frame_height == 0 {
        return None;
    }

    let layout = control_layout(frame_width, frame_height)?;
    let window_x = cursor_x
        .round()
        .clamp(0.0, f64::from(frame_width.saturating_sub(1))) as u32;
    let window_y = cursor_y
        .round()
        .clamp(0.0, f64::from(frame_height.saturating_sub(1))) as u32;

    layout
        .buttons
        .into_iter()
        .find(|button| button.rect.contains(window_x, window_y))
        .map(|button| button.action)
        .or_else(|| {
            seek_preview_at(frame_width, frame_height, cursor_x, cursor_y, snapshot)
                .map(|preview| ControlAction::SeekToRatio(preview.ratio as f32))
        })
}

#[cfg(not(target_os = "macos"))]
pub fn seek_preview_at(
    frame_width: u32,
    frame_height: u32,
    cursor_x: f64,
    cursor_y: f64,
    snapshot: &PlayerSnapshot,
) -> Option<SeekPreview> {
    let layout = control_layout(frame_width, frame_height)?;
    let x = cursor_x
        .round()
        .clamp(0.0, f64::from(frame_width.saturating_sub(1))) as u32;
    let y = cursor_y
        .round()
        .clamp(0.0, f64::from(frame_height.saturating_sub(1))) as u32;
    if !layout.progress_hit_rect.contains(x, y) {
        return None;
    }

    preview_for_progress_ratio(
        snapshot,
        ratio_for_progress_x(layout.progress_rect, cursor_x),
    )
}

#[cfg(not(target_os = "macos"))]
pub fn seek_preview_for_drag(
    frame_width: u32,
    frame_height: u32,
    cursor_x: f64,
    snapshot: &PlayerSnapshot,
) -> Option<SeekPreview> {
    let layout = control_layout(frame_width, frame_height)?;
    preview_for_progress_ratio(
        snapshot,
        ratio_for_progress_x(layout.progress_rect, cursor_x),
    )
}

#[cfg(not(target_os = "macos"))]
fn control_layout(frame_width: u32, frame_height: u32) -> Option<ControlLayout> {
    let metrics = DesktopUiLayoutMetrics::for_surface(frame_width, frame_height)?;
    let bar_rect = DesktopUiRect {
        x: 0,
        y: metrics.overlay_origin_y(frame_height),
        width: frame_width,
        height: metrics.bar_height,
    };
    let progress_rect = metrics.progress_rect(frame_width, frame_height);
    let progress_hit_rect = metrics.progress_hit_rect(frame_width, frame_height);
    let y = metrics.button_origin_y(frame_height);

    let mut buttons = Vec::new();
    let mut x = metrics.padding;
    for (action, visual) in [
        (ControlAction::SeekStart, ControlVisual::SeekStart),
        (ControlAction::SeekBack, ControlVisual::SeekBack),
        (ControlAction::TogglePause, ControlVisual::PlayPause),
        (ControlAction::Stop, ControlVisual::Stop),
        (ControlAction::SeekForward, ControlVisual::SeekForward),
        (ControlAction::SeekEnd, ControlVisual::SeekEnd),
    ] {
        buttons.push(ControlButton {
            rect: DesktopUiRect {
                x,
                y,
                width: metrics.icon_size,
                height: metrics.icon_size,
            },
            action,
            visual,
        });
        x = x.saturating_add(metrics.icon_size + metrics.gap);
    }

    let total_rate_width = CONTROL_RATES.len() as u32 * metrics.rate_width
        + CONTROL_RATES.len().saturating_sub(1) as u32 * metrics.gap;
    let mut rate_x = frame_width.saturating_sub(metrics.padding + total_rate_width);
    for &(rate, label) in CONTROL_RATES {
        buttons.push(ControlButton {
            rect: DesktopUiRect {
                x: rate_x,
                y,
                width: metrics.rate_width,
                height: metrics.icon_size,
            },
            action: ControlAction::SetRate(rate),
            visual: ControlVisual::Rate(label),
        });
        rate_x = rate_x.saturating_add(metrics.rate_width + metrics.gap);
    }

    Some(ControlLayout {
        metrics,
        bar_rect,
        progress_rect,
        progress_hit_rect,
        buttons,
    })
}

#[cfg(not(target_os = "macos"))]
fn draw_control_bar(
    frame: &mut [u8],
    frame_width: u32,
    frame_height: u32,
    view_model: &DesktopUiViewModel,
    layout: &ControlLayout,
) {
    fill_rect(
        frame,
        frame_width,
        frame_height,
        layout.bar_rect,
        [10, 14, 18, 178],
    );

    fill_rect(
        frame,
        frame_width,
        frame_height,
        layout.progress_rect,
        [255, 255, 255, 38],
    );
    if let Some(ratio) = view_model.displayed_progress_ratio {
        let progress_width =
            (ratio.clamp(0.0, 1.0) * f64::from(layout.progress_rect.width)).round() as u32;
        fill_rect(
            frame,
            frame_width,
            frame_height,
            DesktopUiRect {
                x: layout.progress_rect.x,
                width: progress_width,
                ..layout.progress_rect
            },
            [244, 184, 96, 255],
        );
    }

    for button in &layout.buttons {
        let is_active = match button.action {
            ControlAction::TogglePause => true,
            ControlAction::SetRate(rate) => view_model.is_rate_active(rate),
            ControlAction::SeekToRatio(_) => false,
            _ => false,
        };
        let fill = if is_active {
            [244, 184, 96, 238]
        } else {
            [255, 255, 255, 30]
        };
        let border = if is_active {
            [255, 246, 218, 255]
        } else {
            [255, 255, 255, 80]
        };
        let text = if is_active {
            [28, 24, 20, 255]
        } else {
            [244, 246, 248, 255]
        };

        fill_rect(frame, frame_width, frame_height, button.rect, fill);
        stroke_rect(frame, frame_width, frame_height, button.rect, border, 2);

        let label = button_label(*button, view_model.play_pause_label);
        let scale = match button.visual {
            ControlVisual::Rate(_) => 2,
            _ => 3,
        };
        let text_width = measure_text(label, scale);
        let text_height = 7 * scale;
        let text_x = button
            .rect
            .x
            .saturating_add(button.rect.width.saturating_sub(text_width) / 2);
        let text_y = button
            .rect
            .y
            .saturating_add(button.rect.height.saturating_sub(text_height) / 2);
        draw_text(
            frame,
            frame_width,
            frame_height,
            text_x,
            text_y,
            label,
            scale,
            text,
        );
    }

    let time_scale = 2;
    let time_width = measure_text(&view_model.time_label, time_scale);
    let time_x = frame_width.saturating_sub(time_width) / 2;
    let time_y = layout
        .bar_rect
        .y
        .saturating_add(layout.metrics.time_label_offset_y());
    draw_text(
        frame,
        frame_width,
        frame_height,
        time_x,
        time_y,
        &view_model.time_label,
        time_scale,
        [244, 246, 248, 255],
    );
}

#[cfg(not(target_os = "macos"))]
fn preview_for_progress_ratio(snapshot: &PlayerSnapshot, ratio: f64) -> Option<SeekPreview> {
    if !is_scrubbable_timeline(snapshot) {
        return None;
    }

    let clamped_ratio = ratio.clamp(0.0, 1.0);
    let position = snapshot.timeline.position_for_ratio(clamped_ratio)?;
    Some(SeekPreview {
        position,
        ratio: clamped_ratio,
    })
}

#[cfg(not(target_os = "macos"))]
fn ratio_for_progress_x(progress_rect: DesktopUiRect, cursor_x: f64) -> f64 {
    if progress_rect.width == 0 {
        return 0.0;
    }

    ((cursor_x - f64::from(progress_rect.x)) / f64::from(progress_rect.width)).clamp(0.0, 1.0)
}

#[cfg(not(target_os = "macos"))]
fn button_label(button: ControlButton, play_pause_label: &'static str) -> &'static str {
    match button.visual {
        ControlVisual::SeekStart => "|<",
        ControlVisual::SeekBack => "<<",
        ControlVisual::PlayPause => play_pause_label,
        ControlVisual::Stop => "[]",
        ControlVisual::SeekForward => ">>",
        ControlVisual::SeekEnd => ">|",
        ControlVisual::Rate(label) => label,
    }
}

#[cfg(not(target_os = "macos"))]
fn measure_text(text: &str, scale: u32) -> u32 {
    let glyph_width = 5 * scale;
    let spacing = scale;
    text.chars().count() as u32 * (glyph_width + spacing) - spacing.min(glyph_width + spacing)
}

#[cfg(not(target_os = "macos"))]
fn draw_text(
    frame: &mut [u8],
    frame_width: u32,
    frame_height: u32,
    x: u32,
    y: u32,
    text: &str,
    scale: u32,
    color: [u8; 4],
) {
    let glyph_width = 5 * scale;
    let spacing = scale;

    for (index, character) in text.chars().enumerate() {
        let Some(rows) = glyph_rows(character) else {
            continue;
        };
        let glyph_x = x.saturating_add(index as u32 * (glyph_width + spacing));
        for (row_index, row_bits) in rows.iter().enumerate() {
            for column in 0..5u32 {
                if (row_bits >> (4 - column)) & 1 == 0 {
                    continue;
                }

                fill_rect(
                    frame,
                    frame_width,
                    frame_height,
                    DesktopUiRect {
                        x: glyph_x.saturating_add(column * scale),
                        y: y.saturating_add(row_index as u32 * scale),
                        width: scale,
                        height: scale,
                    },
                    color,
                );
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn glyph_rows(character: char) -> Option<[u8; 7]> {
    match character.to_ascii_uppercase() {
        '0' => Some([
            0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110,
        ]),
        '1' => Some([
            0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ]),
        '2' => Some([
            0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111,
        ]),
        '3' => Some([
            0b11110, 0b00001, 0b00001, 0b01110, 0b00001, 0b00001, 0b11110,
        ]),
        '4' => Some([
            0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010,
        ]),
        '5' => Some([
            0b11111, 0b10000, 0b10000, 0b11110, 0b00001, 0b00001, 0b11110,
        ]),
        '6' => Some([
            0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110,
        ]),
        '7' => Some([
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000,
        ]),
        '8' => Some([
            0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110,
        ]),
        '9' => Some([
            0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b11100,
        ]),
        ':' => Some([
            0b00000, 0b00100, 0b00100, 0b00000, 0b00100, 0b00100, 0b00000,
        ]),
        '.' => Some([
            0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00100, 0b00100,
        ]),
        '/' => Some([
            0b00001, 0b00010, 0b00010, 0b00100, 0b01000, 0b01000, 0b10000,
        ]),
        '[' => Some([
            0b01110, 0b01000, 0b01000, 0b01000, 0b01000, 0b01000, 0b01110,
        ]),
        ']' => Some([
            0b01110, 0b00010, 0b00010, 0b00010, 0b00010, 0b00010, 0b01110,
        ]),
        '<' => Some([
            0b00010, 0b00100, 0b01000, 0b10000, 0b01000, 0b00100, 0b00010,
        ]),
        '>' => Some([
            0b01000, 0b00100, 0b00010, 0b00001, 0b00010, 0b00100, 0b01000,
        ]),
        'X' => Some([
            0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b01010, 0b10001,
        ]),
        '|' => Some([
            0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
        ]),
        '-' => Some([
            0b00000, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000, 0b00000,
        ]),
        _ => None,
    }
}

#[cfg(not(target_os = "macos"))]
fn fill_rect(
    frame: &mut [u8],
    frame_width: u32,
    frame_height: u32,
    rect: DesktopUiRect,
    color: [u8; 4],
) {
    let x_end = rect.x.saturating_add(rect.width).min(frame_width);
    let y_end = rect.y.saturating_add(rect.height).min(frame_height);
    for y in rect.y.min(frame_height)..y_end {
        for x in rect.x.min(frame_width)..x_end {
            blend_pixel(frame, frame_width, frame_height, x, y, color);
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn stroke_rect(
    frame: &mut [u8],
    frame_width: u32,
    frame_height: u32,
    rect: DesktopUiRect,
    color: [u8; 4],
    thickness: u32,
) {
    fill_rect(
        frame,
        frame_width,
        frame_height,
        DesktopUiRect {
            x: rect.x,
            y: rect.y,
            width: rect.width,
            height: thickness.min(rect.height),
        },
        color,
    );
    fill_rect(
        frame,
        frame_width,
        frame_height,
        DesktopUiRect {
            x: rect.x,
            y: rect.y.saturating_add(rect.height.saturating_sub(thickness)),
            width: rect.width,
            height: thickness.min(rect.height),
        },
        color,
    );
    fill_rect(
        frame,
        frame_width,
        frame_height,
        DesktopUiRect {
            x: rect.x,
            y: rect.y,
            width: thickness.min(rect.width),
            height: rect.height,
        },
        color,
    );
    fill_rect(
        frame,
        frame_width,
        frame_height,
        DesktopUiRect {
            x: rect.x.saturating_add(rect.width.saturating_sub(thickness)),
            y: rect.y,
            width: thickness.min(rect.width),
            height: rect.height,
        },
        color,
    );
}

#[cfg(not(target_os = "macos"))]
fn blend_pixel(
    frame: &mut [u8],
    frame_width: u32,
    frame_height: u32,
    x: u32,
    y: u32,
    color: [u8; 4],
) {
    if x >= frame_width || y >= frame_height {
        return;
    }

    let index = ((y * frame_width + x) * 4) as usize;
    let alpha = f32::from(color[3]) / 255.0;
    let inverse = 1.0 - alpha;

    frame[index] = (f32::from(color[0]) * alpha + f32::from(frame[index]) * inverse)
        .round()
        .clamp(0.0, 255.0) as u8;
    frame[index + 1] = (f32::from(color[1]) * alpha + f32::from(frame[index + 1]) * inverse)
        .round()
        .clamp(0.0, 255.0) as u8;
    frame[index + 2] = (f32::from(color[2]) * alpha + f32::from(frame[index + 2]) * inverse)
        .round()
        .clamp(0.0, 255.0) as u8;
    frame[index + 3] = 255;
}
