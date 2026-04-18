use player_render_wgpu::RgbaOverlayFrame;
use player_runtime::PlayerSnapshot;

use crate::desktop_symbols::{DesktopSymbol, draw_symbol};
use crate::desktop_ui::{
    CONTROL_RATES, ControlAction, DesktopOverlayViewModel, DesktopSidebarTab,
    DesktopUiLayoutMetrics, DesktopUiRect, DesktopUiViewModel, SeekPreview, format_duration,
    is_scrubbable_timeline,
};

#[derive(Debug, Clone, Copy)]
struct OverlayButton {
    rect: DesktopUiRect,
    action: ControlAction,
    style: ButtonStyle,
}

#[derive(Debug, Clone, Copy)]
enum ButtonStyle {
    Utility,
    TransportSecondary,
    TransportPrimary,
    Rate,
    SidebarTab,
    SourceAction,
    SectionAction,
    CardAction,
}

#[derive(Debug, Clone)]
struct SidebarLayout {
    sidebar_rect: DesktopUiRect,
    stage_width: u32,
    stage_toolbar_rect: DesktopUiRect,
    control_bar_rect: DesktopUiRect,
    progress_rect: DesktopUiRect,
    progress_hit_rect: DesktopUiRect,
    sidebar_padding: u32,
    column_width: u32,
    header_rect: DesktopUiRect,
    content_y: u32,
    buttons: Vec<OverlayButton>,
}

pub fn render_desktop_overlay(
    frame_width: u32,
    frame_height: u32,
    snapshot: &PlayerSnapshot,
    seek_preview: Option<SeekPreview>,
    overlay: &DesktopOverlayViewModel,
) -> Option<RgbaOverlayFrame> {
    if frame_width == 0 || frame_height == 0 {
        return None;
    }

    let layout = overlay_layout(frame_width, frame_height, overlay)?;
    let view_model = DesktopUiViewModel::from_snapshot(snapshot, true, seek_preview);
    let mut overlay_bytes = vec![0; frame_width as usize * frame_height as usize * 4];

    draw_stage_controls(
        &mut overlay_bytes,
        frame_width,
        frame_height,
        &view_model,
        overlay,
        &layout,
    );
    draw_sidebar(
        &mut overlay_bytes,
        frame_width,
        frame_height,
        overlay,
        &layout,
    );

    Some(RgbaOverlayFrame {
        width: frame_width,
        height: frame_height,
        bytes: overlay_bytes,
    })
}

pub fn stage_and_sidebar_rects(
    frame_width: u32,
    frame_height: u32,
) -> Option<(DesktopUiRect, DesktopUiRect)> {
    let sidebar_width = (frame_width / 4).clamp(320, 388);
    let stage_width = frame_width.saturating_sub(sidebar_width);
    if stage_width < 420 {
        return None;
    }
    Some((
        DesktopUiRect {
            x: 0,
            y: 0,
            width: stage_width,
            height: frame_height,
        },
        DesktopUiRect {
            x: stage_width,
            y: 0,
            width: sidebar_width,
            height: frame_height,
        },
    ))
}

pub fn playback_stage_rect(frame_width: u32, frame_height: u32) -> DesktopUiRect {
    stage_and_sidebar_rects(frame_width, frame_height)
        .map(|(stage_rect, _)| stage_rect)
        .unwrap_or(DesktopUiRect {
            x: 0,
            y: 0,
            width: frame_width,
            height: frame_height,
        })
}

pub fn overlay_action_at(
    frame_width: u32,
    frame_height: u32,
    cursor_x: f64,
    cursor_y: f64,
    snapshot: &PlayerSnapshot,
    overlay: &DesktopOverlayViewModel,
) -> Option<ControlAction> {
    let layout = overlay_layout(frame_width, frame_height, overlay)?;
    let x = clamp_cursor(cursor_x, frame_width);
    let y = clamp_cursor(cursor_y, frame_height);
    let stage_controls_interactive = stage_controls_interactive(overlay);

    layout
        .buttons
        .iter()
        .find(|button| {
            button.rect.contains(x, y)
                && (button.rect.x >= layout.sidebar_rect.x || stage_controls_interactive)
        })
        .map(|button| button.action)
        .or_else(|| {
            seek_preview_at(
                frame_width,
                frame_height,
                cursor_x,
                cursor_y,
                snapshot,
                overlay,
            )
            .map(|preview| ControlAction::SeekToRatio(preview.ratio as f32))
        })
}

pub fn seek_preview_at(
    frame_width: u32,
    frame_height: u32,
    cursor_x: f64,
    cursor_y: f64,
    snapshot: &PlayerSnapshot,
    overlay: &DesktopOverlayViewModel,
) -> Option<SeekPreview> {
    if !stage_controls_interactive(overlay) {
        return None;
    }
    let layout = overlay_layout(frame_width, frame_height, overlay)?;
    let x = clamp_cursor(cursor_x, frame_width);
    let y = clamp_cursor(cursor_y, frame_height);
    if !layout.progress_hit_rect.contains(x, y) {
        return None;
    }
    preview_for_progress_ratio(
        snapshot,
        ratio_for_progress_x(layout.progress_rect, cursor_x),
    )
}

pub fn seek_preview_for_drag(
    frame_width: u32,
    frame_height: u32,
    cursor_x: f64,
    snapshot: &PlayerSnapshot,
    overlay: &DesktopOverlayViewModel,
) -> Option<SeekPreview> {
    if !stage_controls_interactive(overlay) {
        return None;
    }
    let layout = overlay_layout(frame_width, frame_height, overlay)?;
    preview_for_progress_ratio(
        snapshot,
        ratio_for_progress_x(layout.progress_rect, cursor_x),
    )
}

fn overlay_layout(
    frame_width: u32,
    frame_height: u32,
    overlay: &DesktopOverlayViewModel,
) -> Option<SidebarLayout> {
    let (stage_rect, sidebar_rect) = stage_and_sidebar_rects(frame_width, frame_height)?;
    let stage_width = stage_rect.width;
    let stage_toolbar_rect = DesktopUiRect {
        x: stage_rect.x.saturating_add(22),
        y: 20,
        width: stage_width.saturating_sub(44),
        height: 48,
    };

    let metrics = DesktopUiLayoutMetrics::for_surface(stage_width, frame_height)?;
    let control_bar_width = stage_width
        .saturating_mul(34)
        .saturating_div(100)
        .clamp(300, 420)
        .min(stage_width.saturating_sub(48));
    let control_bar_height = 84;
    let control_bar_rect = DesktopUiRect {
        x: stage_width
            .saturating_div(2)
            .saturating_sub(control_bar_width.saturating_div(2)),
        y: frame_height
            .saturating_sub(control_bar_height)
            .saturating_sub(28),
        width: control_bar_width,
        height: control_bar_height,
    };
    let mut buttons = Vec::new();
    let control_content_rect = DesktopUiRect {
        x: control_bar_rect.x.saturating_add(16),
        y: control_bar_rect.y.saturating_add(14),
        width: control_bar_rect.width.saturating_sub(32),
        height: control_bar_rect.height.saturating_sub(28),
    };

    let rate_gap = (metrics.gap / 2).max(6);
    let rate_height = 20;
    let top_row_y = control_bar_rect
        .y
        .saturating_sub(rate_height)
        .saturating_sub(10);
    let rate_widths = CONTROL_RATES
        .iter()
        .map(|(_, label)| measure_text(label, 1).saturating_add(14).max(32))
        .collect::<Vec<_>>();
    let total_rate_width = rate_widths.iter().sum::<u32>()
        + rate_gap.saturating_mul(rate_widths.len().saturating_sub(1) as u32);
    let mut rate_x = control_bar_rect
        .x
        .saturating_add(control_bar_rect.width.saturating_sub(total_rate_width) / 2);
    for ((&(rate, _), width), index) in CONTROL_RATES
        .iter()
        .zip(rate_widths.iter().copied())
        .zip(0..CONTROL_RATES.len())
    {
        buttons.push(OverlayButton {
            rect: DesktopUiRect {
                x: rate_x,
                y: top_row_y,
                width,
                height: rate_height,
            },
            action: ControlAction::SetRate(rate),
            style: ButtonStyle::Rate,
        });
        if index + 1 < CONTROL_RATES.len() {
            rate_x = rate_x.saturating_add(width + rate_gap);
        }
    }

    let progress_label_width = measure_text("00:00", 1).saturating_add(10);
    let progress_rect = DesktopUiRect {
        x: control_content_rect
            .x
            .saturating_add(progress_label_width)
            .saturating_add(10),
        y: control_bar_rect
            .y
            .saturating_add(control_bar_rect.height)
            .saturating_sub(24),
        width: control_content_rect
            .width
            .saturating_sub(progress_label_width.saturating_mul(2))
            .saturating_sub(20),
        height: metrics.progress_height.max(4),
    };
    let progress_hit_top = metrics.progress_hit_slop_top.saturating_add(4);
    let progress_hit_bottom = metrics.progress_hit_slop_bottom.saturating_add(8);
    let progress_hit_y = progress_rect.y.saturating_sub(progress_hit_top);
    let progress_hit_bottom_y = progress_rect
        .y
        .saturating_add(progress_rect.height)
        .saturating_add(progress_hit_bottom)
        .min(frame_height);
    let progress_hit_rect = DesktopUiRect {
        x: progress_rect.x,
        y: progress_hit_y,
        width: progress_rect.width,
        height: progress_hit_bottom_y.saturating_sub(progress_hit_y),
    };

    let primary_size = 28;
    let secondary_size = 22;
    let utility_size = 18;
    let control_gap = 10;
    let utility_gap = 8;
    let transport_top_y = control_bar_rect.y.saturating_add(16);
    let button_specs = [
        (ControlAction::SeekStart, ButtonStyle::Utility, utility_size),
        (
            ControlAction::SeekBack,
            ButtonStyle::TransportSecondary,
            secondary_size,
        ),
        (
            ControlAction::TogglePause,
            ButtonStyle::TransportPrimary,
            primary_size,
        ),
        (
            ControlAction::SeekForward,
            ButtonStyle::TransportSecondary,
            secondary_size,
        ),
        (ControlAction::SeekEnd, ButtonStyle::Utility, utility_size),
        (ControlAction::Stop, ButtonStyle::Utility, utility_size),
    ];
    let transport_width =
        button_specs
            .iter()
            .enumerate()
            .fold(0_u32, |width, (index, (_, style, size))| {
                let gap = if index == 0 {
                    0
                } else if matches!(style, ButtonStyle::Utility) {
                    utility_gap
                } else {
                    control_gap
                };
                width.saturating_add(gap).saturating_add(*size)
            });
    let mut current_x = control_bar_rect
        .x
        .saturating_add(control_bar_rect.width.saturating_sub(transport_width) / 2);
    for (index, (action, style, size)) in button_specs.iter().enumerate() {
        if index > 0 {
            let gap = if matches!(style, ButtonStyle::Utility) {
                utility_gap
            } else {
                control_gap
            };
            current_x = current_x.saturating_add(gap);
        }
        let y = transport_top_y.saturating_add(primary_size.saturating_sub(*size) / 2);
        buttons.push(OverlayButton {
            rect: DesktopUiRect {
                x: current_x,
                y,
                width: *size,
                height: *size,
            },
            action: *action,
            style: *style,
        });
        current_x = current_x.saturating_add(*size);
    }

    let sidebar_padding = 18;
    let column_width = sidebar_rect.width.saturating_sub(sidebar_padding * 2);
    let header_rect = DesktopUiRect {
        x: sidebar_rect.x + sidebar_padding,
        y: 18,
        width: column_width,
        height: 56,
    };
    let source_actions_y = header_rect.y.saturating_add(header_rect.height + 18);
    let tab_bar_y = source_actions_y.saturating_add(44);
    let content_y = tab_bar_y.saturating_add(44);

    let source_button_gap = 10;
    let source_button_width = (column_width.saturating_sub(source_button_gap * 2)) / 3;
    for (index, action) in [
        ControlAction::OpenLocalFile,
        ControlAction::OpenHlsDemo,
        ControlAction::OpenDashDemo,
    ]
    .iter()
    .enumerate()
    {
        buttons.push(OverlayButton {
            rect: DesktopUiRect {
                x: sidebar_rect.x
                    + sidebar_padding
                    + index as u32 * (source_button_width + source_button_gap),
                y: source_actions_y,
                width: source_button_width,
                height: 34,
            },
            action: *action,
            style: ButtonStyle::SourceAction,
        });
    }

    let tab_gap = 8;
    let tab_width = (column_width.saturating_sub(tab_gap)) / 2;
    for (index, tab) in [DesktopSidebarTab::Playlist, DesktopSidebarTab::Downloads]
        .iter()
        .enumerate()
    {
        buttons.push(OverlayButton {
            rect: DesktopUiRect {
                x: sidebar_rect.x + sidebar_padding + index as u32 * (tab_width + tab_gap),
                y: tab_bar_y,
                width: tab_width,
                height: 32,
            },
            action: ControlAction::SelectSidebarTab(*tab),
            style: ButtonStyle::SidebarTab,
        });
    }

    match overlay.sidebar_tab {
        DesktopSidebarTab::Playlist => {
            let mut current_y = content_y.saturating_add(26);
            for (index, _) in overlay.playlist_items.iter().enumerate() {
                buttons.push(OverlayButton {
                    rect: DesktopUiRect {
                        x: sidebar_rect.x + sidebar_padding,
                        y: current_y,
                        width: column_width,
                        height: 58,
                    },
                    action: ControlAction::FocusPlaylistItem(index),
                    style: ButtonStyle::SectionAction,
                });
                current_y = current_y.saturating_add(68);
            }
        }
        DesktopSidebarTab::Downloads => {
            let download_actions_y = content_y.saturating_add(24);
            let download_button_width = (column_width.saturating_sub(source_button_gap * 2)) / 3;
            for (index, action) in [
                ControlAction::CreateDownloadCurrentSource,
                ControlAction::CreateDownloadHlsDemo,
                ControlAction::CreateDownloadDashDemo,
            ]
            .iter()
            .enumerate()
            {
                buttons.push(OverlayButton {
                    rect: DesktopUiRect {
                        x: sidebar_rect.x
                            + sidebar_padding
                            + index as u32 * (download_button_width + source_button_gap),
                        y: download_actions_y,
                        width: download_button_width,
                        height: 32,
                    },
                    action: *action,
                    style: ButtonStyle::SectionAction,
                });
            }

            let mut current_y = download_actions_y.saturating_add(56);
            current_y = current_y.saturating_add(overlay.pending_downloads.len() as u32 * 44);

            for task in &overlay.download_tasks {
                let card_rect = DesktopUiRect {
                    x: sidebar_rect.x + sidebar_padding,
                    y: current_y,
                    width: column_width,
                    height: if task.primary_action_label.is_some()
                        || task.export_action_label.is_some()
                    {
                        118
                    } else {
                        92
                    },
                };
                let action_y = card_rect.y + card_rect.height.saturating_sub(34);
                let action_width = (column_width.saturating_sub(20)) / 3;
                if task.primary_action_label.is_some() {
                    buttons.push(OverlayButton {
                        rect: DesktopUiRect {
                            x: card_rect.x + 10,
                            y: action_y,
                            width: action_width,
                            height: 24,
                        },
                        action: ControlAction::DownloadPrimaryAction(task.task_id),
                        style: ButtonStyle::CardAction,
                    });
                }
                if task.export_action_label.is_some() {
                    buttons.push(OverlayButton {
                        rect: DesktopUiRect {
                            x: card_rect.x + 10 + action_width + 10,
                            y: action_y,
                            width: action_width,
                            height: 24,
                        },
                        action: ControlAction::DownloadExport(task.task_id),
                        style: ButtonStyle::CardAction,
                    });
                }
                buttons.push(OverlayButton {
                    rect: DesktopUiRect {
                        x: card_rect.x + 10 + (action_width + 10) * 2,
                        y: action_y,
                        width: action_width,
                        height: 24,
                    },
                    action: ControlAction::DownloadRemove(task.task_id),
                    style: ButtonStyle::CardAction,
                });
                current_y = current_y.saturating_add(card_rect.height + 14);
            }
        }
    }

    Some(SidebarLayout {
        sidebar_rect,
        stage_width,
        stage_toolbar_rect,
        control_bar_rect,
        progress_rect,
        progress_hit_rect,
        sidebar_padding,
        column_width,
        header_rect,
        content_y,
        buttons,
    })
}

fn draw_stage_controls(
    frame: &mut [u8],
    frame_width: u32,
    frame_height: u32,
    view_model: &DesktopUiViewModel,
    overlay: &DesktopOverlayViewModel,
    layout: &SidebarLayout,
) {
    let control_content_rect = DesktopUiRect {
        x: layout.control_bar_rect.x.saturating_add(16),
        y: layout.control_bar_rect.y.saturating_add(14),
        width: layout.control_bar_rect.width.saturating_sub(32),
        height: layout.control_bar_rect.height.saturating_sub(28),
    };
    let controls_opacity = overlay.controls_opacity.clamp(0.0, 1.0);
    let hovered_action = hovered_action_for_layout(layout, overlay);

    if controls_opacity > 0.01 {
        draw_stage_toolbar(
            frame,
            frame_width,
            frame_height,
            overlay,
            layout,
            controls_opacity,
        );
    }
    if controls_opacity > 0.01 {
        fill_rounded_rect(
            frame,
            frame_width,
            frame_height,
            layout.control_bar_rect,
            18,
            scale_alpha([30, 30, 32, 214], controls_opacity),
        );
    }

    if controls_opacity > 0.01 {
        fill_rounded_rect(
            frame,
            frame_width,
            frame_height,
            layout.progress_rect,
            2,
            scale_alpha([255, 255, 255, 24], controls_opacity),
        );
        if let Some(ratio) = view_model.displayed_progress_ratio {
            let progress_width =
                (ratio.clamp(0.0, 1.0) * f64::from(layout.progress_rect.width)).round() as u32;
            fill_rounded_rect(
                frame,
                frame_width,
                frame_height,
                DesktopUiRect {
                    x: layout.progress_rect.x,
                    y: layout.progress_rect.y,
                    width: progress_width,
                    height: layout.progress_rect.height,
                },
                2,
                scale_alpha([244, 244, 244, 255], controls_opacity),
            );
            let knob_radius = 5;
            let knob_center_x = layout.progress_rect.x.saturating_add(
                progress_width
                    .max(knob_radius)
                    .min(layout.progress_rect.width.saturating_sub(knob_radius)),
            );
            fill_circle(
                frame,
                frame_width,
                frame_height,
                knob_center_x,
                layout.progress_rect.y + layout.progress_rect.height / 2,
                knob_radius,
                scale_alpha([255, 255, 255, 250], controls_opacity),
            );
        }

        let current_time = format_duration(view_model.displayed_position);
        let duration_time = view_model
            .duration
            .map(format_duration)
            .unwrap_or_else(|| "--:--".to_owned());
        let bottom_text_y = layout.progress_rect.y.saturating_sub(2);
        draw_text(
            frame,
            frame_width,
            frame_height,
            control_content_rect.x,
            bottom_text_y,
            &current_time,
            1,
            scale_alpha([180, 180, 184, 255], controls_opacity),
        );
        let duration_width = measure_text(&duration_time, 1);
        draw_text(
            frame,
            frame_width,
            frame_height,
            control_content_rect
                .x
                .saturating_add(control_content_rect.width)
                .saturating_sub(duration_width),
            bottom_text_y,
            &duration_time,
            1,
            scale_alpha([180, 180, 184, 255], controls_opacity),
        );
    }

    for button in &layout.buttons {
        if button.rect.x >= layout.sidebar_rect.x {
            continue;
        }
        if controls_opacity <= 0.01 {
            continue;
        }
        let is_hovered = hovered_action == Some(button.action);
        let is_active = match button.action {
            ControlAction::SetRate(rate) => view_model.is_rate_active(rate),
            _ => false,
        };
        let (fill, border, text, scale) = match button.style {
            ButtonStyle::Utility if is_hovered => {
                ([255, 255, 255, 18], [0, 0, 0, 0], [255, 255, 255, 255], 1)
            }
            ButtonStyle::Utility => ([0, 0, 0, 0], [0, 0, 0, 0], [208, 214, 224, 255], 1),
            ButtonStyle::TransportSecondary if is_hovered => (
                [255, 255, 255, 18],
                [255, 255, 255, 18],
                [255, 255, 255, 255],
                2,
            ),
            ButtonStyle::TransportSecondary => {
                ([0, 0, 0, 0], [0, 0, 0, 0], [244, 246, 248, 255], 2)
            }
            ButtonStyle::TransportPrimary if is_hovered => (
                [255, 255, 255, 22],
                [255, 255, 255, 22],
                [255, 255, 255, 255],
                2,
            ),
            ButtonStyle::TransportPrimary => ([0, 0, 0, 0], [0, 0, 0, 0], [244, 246, 248, 255], 2),
            ButtonStyle::Rate if is_active => {
                ([250, 250, 250, 238], [0, 0, 0, 0], [24, 24, 24, 255], 1)
            }
            ButtonStyle::Rate if is_hovered => {
                ([255, 255, 255, 18], [0, 0, 0, 0], [244, 246, 248, 255], 1)
            }
            ButtonStyle::Rate => ([255, 255, 255, 8], [0, 0, 0, 0], [178, 188, 200, 255], 1),
            _ => (
                [255, 255, 255, 18],
                [255, 255, 255, 46],
                [244, 246, 248, 255],
                1,
            ),
        };
        let fill = scale_alpha(fill, controls_opacity);
        let border = scale_alpha(border, controls_opacity);
        let text = scale_alpha(text, controls_opacity);

        if matches!(button.style, ButtonStyle::Rate) {
            fill_rounded_rect(frame, frame_width, frame_height, button.rect, 8, fill);
        } else if fill[3] > 0 {
            fill_rounded_rect(
                frame,
                frame_width,
                frame_height,
                button.rect,
                button.rect.height.saturating_div(2).max(8),
                fill,
            );
        }
        if border[3] > 0 {
            stroke_rounded_rect(
                frame,
                frame_width,
                frame_height,
                button.rect,
                button.rect.height.saturating_div(2).max(8),
                border,
                1,
            );
        }
        match button.style {
            ButtonStyle::Utility
            | ButtonStyle::TransportSecondary
            | ButtonStyle::TransportPrimary => {
                if let Some(symbol) = control_action_symbol(button.action, view_model.is_playing) {
                    draw_symbol(frame, frame_width, frame_height, button.rect, symbol, text);
                }
            }
            ButtonStyle::Rate => draw_centered_text(
                frame,
                frame_width,
                frame_height,
                button.rect,
                control_button_label(button.action, view_model.play_pause_label),
                scale,
                text,
            ),
            _ => {}
        }
    }
    if let Some(message) = overlay.host_message.as_deref() {
        draw_host_message(
            frame,
            frame_width,
            frame_height,
            DesktopUiRect {
                x: layout.stage_width.saturating_div(2).saturating_sub(170),
                y: frame_height.saturating_div(2).saturating_sub(44),
                width: 340.min(layout.stage_width.saturating_sub(48)),
                height: 88,
            },
            message,
        );
    }
}

fn draw_sidebar(
    frame: &mut [u8],
    frame_width: u32,
    frame_height: u32,
    overlay: &DesktopOverlayViewModel,
    layout: &SidebarLayout,
) {
    fill_vertical_gradient(
        frame,
        frame_width,
        frame_height,
        layout.sidebar_rect,
        [12, 14, 20, 244],
        [8, 10, 16, 252],
    );
    stroke_rect(
        frame,
        frame_width,
        frame_height,
        DesktopUiRect {
            x: layout.sidebar_rect.x,
            y: 0,
            width: 1,
            height: layout.sidebar_rect.height,
        },
        [255, 255, 255, 36],
        1,
    );

    let hovered_action = hovered_action_for_layout(layout, overlay);
    let protocol = overlay_source_protocol(overlay);
    let protocol_color = protocol_accent(protocol);
    fill_rounded_rect(
        frame,
        frame_width,
        frame_height,
        layout.header_rect,
        16,
        [16, 20, 28, 228],
    );
    stroke_rounded_rect(
        frame,
        frame_width,
        frame_height,
        layout.header_rect,
        16,
        [255, 255, 255, 18],
        1,
    );
    let hero_icon_rect = DesktopUiRect {
        x: layout.header_rect.x + 16,
        y: layout.header_rect.y + 12,
        width: 34,
        height: 34,
    };
    fill_rounded_rect(
        frame,
        frame_width,
        frame_height,
        hero_icon_rect,
        10,
        tint(protocol_color, 34),
    );
    stroke_rounded_rect(
        frame,
        frame_width,
        frame_height,
        hero_icon_rect,
        10,
        tint(protocol_color, 92),
        1,
    );
    draw_symbol(
        frame,
        frame_width,
        frame_height,
        DesktopUiRect {
            x: hero_icon_rect.x + 7,
            y: hero_icon_rect.y + 7,
            width: 20,
            height: 20,
        },
        DesktopSymbol::Magic,
        [255, 247, 232, 255],
    );
    let header_text_x = hero_icon_rect.x + hero_icon_rect.width + 12;
    let active_chip_label = match overlay.sidebar_tab {
        DesktopSidebarTab::Playlist => "QUEUE",
        DesktopSidebarTab::Downloads => "EXPORTS",
    };
    let active_chip_width = measure_text(active_chip_label, 1)
        .saturating_add(34)
        .max(76);
    let active_chip = DesktopUiRect {
        x: layout.header_rect.x.saturating_add(
            layout
                .header_rect
                .width
                .saturating_sub(active_chip_width + 16),
        ),
        y: layout.header_rect.y + 16,
        width: active_chip_width,
        height: 20,
    };
    draw_text(
        frame,
        frame_width,
        frame_height,
        header_text_x,
        layout.header_rect.y + 12,
        "MEDIA PANEL",
        2,
        [255, 255, 255, 255],
    );
    let meta_width = active_chip
        .x
        .saturating_sub(header_text_x)
        .saturating_sub(10);
    draw_text(
        frame,
        frame_width,
        frame_height,
        header_text_x,
        layout.header_rect.y + 34,
        &fit_text_to_width(
            &format!("{} {}", overlay.playback_state_label, overlay.subtitle),
            1,
            1,
            meta_width,
        )
        .0,
        1,
        [162, 173, 189, 255],
    );
    draw_badge_with_symbol(
        frame,
        frame_width,
        frame_height,
        active_chip,
        match overlay.sidebar_tab {
            DesktopSidebarTab::Playlist => DesktopSymbol::Playlist,
            DesktopSidebarTab::Downloads => DesktopSymbol::Download,
        },
        active_chip_label,
        [244, 184, 96, 38],
        [244, 184, 96, 255],
        [255, 248, 236, 255],
    );

    let source_row_top = layout
        .buttons
        .iter()
        .filter(|button| matches!(button.style, ButtonStyle::SourceAction))
        .map(|button| button.rect.y)
        .min()
        .unwrap_or(layout.content_y);
    let source_header_y = source_row_top.saturating_sub(18);
    draw_symbol(
        frame,
        frame_width,
        frame_height,
        DesktopUiRect {
            x: layout.sidebar_rect.x + layout.sidebar_padding,
            y: source_header_y.saturating_sub(2),
            width: 14,
            height: 14,
        },
        DesktopSymbol::Waveform,
        [192, 202, 214, 255],
    );
    draw_text(
        frame,
        frame_width,
        frame_height,
        layout.sidebar_rect.x + layout.sidebar_padding + 20,
        source_header_y,
        "QUICK SOURCES",
        1,
        [162, 173, 189, 255],
    );
    let protocol_chip_width = measure_text(protocol, 1).saturating_add(34).max(68);
    draw_badge_with_symbol(
        frame,
        frame_width,
        frame_height,
        DesktopUiRect {
            x: layout
                .sidebar_rect
                .x
                .saturating_add(layout.column_width.saturating_sub(protocol_chip_width))
                .saturating_add(layout.sidebar_padding),
            y: source_header_y.saturating_sub(5),
            width: protocol_chip_width,
            height: 20,
        },
        source_protocol_symbol(protocol),
        protocol,
        tint(protocol_color, 30),
        protocol_color,
        [244, 248, 252, 255],
    );

    for button in &layout.buttons {
        if button.rect.x < layout.sidebar_rect.x {
            continue;
        }
        if !matches!(
            button.style,
            ButtonStyle::SourceAction | ButtonStyle::SidebarTab
        ) {
            continue;
        }
        let is_hovered = hovered_action == Some(button.action);
        let is_active = match button.action {
            ControlAction::SelectSidebarTab(tab) => tab == overlay.sidebar_tab,
            _ => false,
        };
        let accent = action_accent(button.action);
        let (fill, border, icon, text) = match button.style {
            ButtonStyle::SidebarTab if is_active => (
                tint(accent, 34),
                tint(accent, 116),
                accent,
                [255, 248, 236, 255],
            ),
            ButtonStyle::SidebarTab if is_hovered => (
                tint(accent, 18),
                tint(accent, 68),
                accent,
                [255, 255, 255, 255],
            ),
            ButtonStyle::SidebarTab => (
                [255, 255, 255, 8],
                [255, 255, 255, 18],
                tint(accent, 232),
                [192, 201, 212, 255],
            ),
            ButtonStyle::SourceAction if is_hovered => (
                tint(accent, 20),
                tint(accent, 82),
                accent,
                [255, 255, 255, 255],
            ),
            ButtonStyle::SourceAction => (
                tint(accent, 14),
                tint(accent, 46),
                tint(accent, 236),
                [236, 241, 248, 255],
            ),
            _ => (
                [255, 255, 255, 12],
                [255, 255, 255, 22],
                [232, 238, 246, 255],
                [232, 238, 246, 255],
            ),
        };
        fill_rounded_rect(frame, frame_width, frame_height, button.rect, 12, fill);
        stroke_rounded_rect(frame, frame_width, frame_height, button.rect, 12, border, 1);
        draw_centered_symbol_label_tones(
            frame,
            frame_width,
            frame_height,
            button.rect,
            action_symbol(button.action, false),
            sidebar_button_label(button.action),
            1,
            icon,
            text,
        );
    }

    match overlay.sidebar_tab {
        DesktopSidebarTab::Playlist => {
            draw_playlist_section(
                frame,
                frame_width,
                frame_height,
                overlay,
                layout,
                hovered_action,
            );
        }
        DesktopSidebarTab::Downloads => {
            draw_downloads_section(
                frame,
                frame_width,
                frame_height,
                overlay,
                layout,
                hovered_action,
            );
        }
    }
}

fn draw_playlist_section(
    frame: &mut [u8],
    frame_width: u32,
    frame_height: u32,
    overlay: &DesktopOverlayViewModel,
    layout: &SidebarLayout,
    hovered_action: Option<ControlAction>,
) {
    let left = layout.sidebar_rect.x + layout.sidebar_padding;
    let width = layout.column_width;
    let section_title_y = layout.content_y;
    let section_rect = DesktopUiRect {
        x: left.saturating_sub(8),
        y: layout.content_y.saturating_sub(18),
        width: width.saturating_add(16),
        height: if overlay.playlist_items.is_empty() {
            124
        } else {
            (overlay.playlist_items.len() as u32)
                .saturating_mul(68)
                .saturating_add(58)
        },
    };
    fill_rounded_rect(
        frame,
        frame_width,
        frame_height,
        section_rect,
        18,
        [255, 255, 255, 6],
    );
    stroke_rounded_rect(
        frame,
        frame_width,
        frame_height,
        section_rect,
        18,
        [255, 255, 255, 18],
        1,
    );
    draw_symbol(
        frame,
        frame_width,
        frame_height,
        DesktopUiRect {
            x: left,
            y: section_title_y.saturating_sub(2),
            width: 14,
            height: 14,
        },
        DesktopSymbol::Playlist,
        [244, 184, 96, 255],
    );
    draw_text(
        frame,
        frame_width,
        frame_height,
        left + 20,
        section_title_y,
        "PLAYLIST",
        1,
        [162, 173, 189, 255],
    );
    draw_badge(
        frame,
        frame_width,
        frame_height,
        DesktopUiRect {
            x: left + width.saturating_sub(74),
            y: section_title_y.saturating_sub(5),
            width: 74,
            height: 20,
        },
        &format!("{} ITEMS", overlay.playlist_items.len()),
        [130, 208, 166, 28],
        [130, 208, 166, 255],
    );

    if overlay.playlist_items.is_empty() {
        let empty_rect = DesktopUiRect {
            x: left,
            y: layout.content_y.saturating_add(28),
            width,
            height: 74,
        };
        fill_rounded_rect(
            frame,
            frame_width,
            frame_height,
            empty_rect,
            14,
            [255, 255, 255, 8],
        );
        draw_centered_text(
            frame,
            frame_width,
            frame_height,
            empty_rect,
            "NO SOURCES IN QUEUE",
            1,
            [170, 182, 195, 255],
        );
        return;
    }

    let mut y = layout.content_y.saturating_add(26);
    for (index, item) in overlay.playlist_items.iter().enumerate() {
        let rect = DesktopUiRect {
            x: left,
            y,
            width,
            height: 58,
        };
        let hovered = hovered_action == Some(ControlAction::FocusPlaylistItem(index));
        let accent = action_accent(ControlAction::FocusPlaylistItem(index));
        fill_rounded_rect(
            frame,
            frame_width,
            frame_height,
            rect,
            14,
            if item.is_active {
                tint(accent, 32)
            } else if hovered {
                [255, 255, 255, 16]
            } else {
                [255, 255, 255, 10]
            },
        );
        stroke_rounded_rect(
            frame,
            frame_width,
            frame_height,
            rect,
            14,
            if item.is_active {
                tint(accent, 128)
            } else if hovered {
                tint(accent, 52)
            } else {
                [255, 255, 255, 22]
            },
            1,
        );
        if item.is_active {
            fill_rounded_rect(
                frame,
                frame_width,
                frame_height,
                DesktopUiRect {
                    x: rect.x + 8,
                    y: rect.y + 10,
                    width: 3,
                    height: rect.height.saturating_sub(20),
                },
                2,
                accent,
            );
        }
        let icon_rect = DesktopUiRect {
            x: rect.x + 14,
            y: rect.y + 13,
            width: 22,
            height: 22,
        };
        fill_rounded_rect(
            frame,
            frame_width,
            frame_height,
            icon_rect,
            8,
            if item.is_active {
                tint(accent, 28)
            } else {
                [255, 255, 255, 12]
            },
        );
        draw_symbol(
            frame,
            frame_width,
            frame_height,
            DesktopUiRect {
                x: icon_rect.x + 4,
                y: icon_rect.y + 4,
                width: 14,
                height: 14,
            },
            DesktopSymbol::VideoStack,
            if item.is_active {
                accent
            } else {
                [206, 214, 224, 255]
            },
        );
        draw_text(
            frame,
            frame_width,
            frame_height,
            rect.x + 46,
            rect.y + 11,
            &normalize_text(&item.label, 24),
            1,
            [255, 255, 255, 255],
        );
        draw_badge(
            frame,
            frame_width,
            frame_height,
            DesktopUiRect {
                x: rect.x + rect.width.saturating_sub(82),
                y: rect.y + 10,
                width: 70,
                height: 18,
            },
            &item.status,
            if item.is_active {
                tint(accent, 28)
            } else {
                [255, 255, 255, 12]
            },
            if item.is_active {
                accent
            } else {
                [170, 182, 195, 255]
            },
        );
        draw_text(
            frame,
            frame_width,
            frame_height,
            rect.x + 46,
            rect.y + 33,
            if item.is_active {
                "ACTIVE SOURCE"
            } else if hovered {
                "CLICK TO SWITCH"
            } else {
                "READY TO OPEN"
            },
            1,
            [148, 159, 176, 255],
        );
        y = y.saturating_add(68);
    }
}

fn draw_downloads_section(
    frame: &mut [u8],
    frame_width: u32,
    frame_height: u32,
    overlay: &DesktopOverlayViewModel,
    layout: &SidebarLayout,
    hovered_action: Option<ControlAction>,
) {
    let left = layout.sidebar_rect.x + layout.sidebar_padding;
    let width = layout.column_width;
    let action_gap = 10;
    let action_width = (width.saturating_sub(action_gap * 2)) / 3;
    let actions_y = layout.content_y.saturating_add(24);
    let mut y = actions_y.saturating_add(56);
    let tasks_total_height = overlay.download_tasks.iter().fold(0_u32, |sum, task| {
        sum.saturating_add(
            if task.primary_action_label.is_some() || task.export_action_label.is_some() {
                132_u32
            } else {
                106_u32
            }
            .saturating_add(14),
        )
    });
    let pending_height = if overlay.pending_downloads.is_empty() {
        0_u32
    } else {
        24_u32.saturating_add((overlay.pending_downloads.len() as u32).saturating_mul(44))
    };
    let empty_height = if overlay.pending_downloads.is_empty() && overlay.download_tasks.is_empty()
    {
        70_u32
    } else {
        0_u32
    };
    let section_rect = DesktopUiRect {
        x: left.saturating_sub(8),
        y: layout.content_y.saturating_sub(18),
        width: width.saturating_add(16),
        height: 126_u32
            .saturating_add(pending_height)
            .saturating_add(empty_height)
            .saturating_add(tasks_total_height)
            .min(
                layout
                    .sidebar_rect
                    .height
                    .saturating_sub(layout.content_y)
                    .saturating_sub(24),
            ),
    };
    fill_rounded_rect(
        frame,
        frame_width,
        frame_height,
        section_rect,
        18,
        [255, 255, 255, 6],
    );
    stroke_rounded_rect(
        frame,
        frame_width,
        frame_height,
        section_rect,
        18,
        [255, 255, 255, 18],
        1,
    );

    draw_symbol(
        frame,
        frame_width,
        frame_height,
        DesktopUiRect {
            x: left,
            y: layout.content_y.saturating_sub(2),
            width: 14,
            height: 14,
        },
        DesktopSymbol::Download,
        [130, 208, 166, 255],
    );
    draw_text(
        frame,
        frame_width,
        frame_height,
        left + 20,
        layout.content_y,
        "DOWNLOAD MANAGER",
        1,
        [162, 173, 189, 255],
    );
    draw_badge(
        frame,
        frame_width,
        frame_height,
        DesktopUiRect {
            x: left + width.saturating_sub(98),
            y: layout.content_y.saturating_sub(5),
            width: 98,
            height: 20,
        },
        if overlay.export_plugin_installed {
            "EXPORT READY"
        } else {
            "EXPORT OFF"
        },
        if overlay.export_plugin_installed {
            [130, 208, 166, 28]
        } else {
            [240, 128, 128, 30]
        },
        if overlay.export_plugin_installed {
            [130, 208, 166, 255]
        } else {
            [240, 128, 128, 255]
        },
    );

    for (index, action) in [
        ControlAction::CreateDownloadCurrentSource,
        ControlAction::CreateDownloadHlsDemo,
        ControlAction::CreateDownloadDashDemo,
    ]
    .iter()
    .enumerate()
    {
        let rect = DesktopUiRect {
            x: left + index as u32 * (action_width + action_gap),
            y: actions_y,
            width: action_width,
            height: 32,
        };
        let hovered = hovered_action == Some(*action);
        let accent = action_accent(*action);
        fill_rounded_rect(
            frame,
            frame_width,
            frame_height,
            rect,
            12,
            if hovered {
                tint(accent, 22)
            } else {
                tint(accent, 14)
            },
        );
        stroke_rounded_rect(
            frame,
            frame_width,
            frame_height,
            rect,
            12,
            if hovered {
                tint(accent, 72)
            } else {
                tint(accent, 44)
            },
            1,
        );
        draw_centered_symbol_label_tones(
            frame,
            frame_width,
            frame_height,
            rect,
            action_symbol(*action, false),
            sidebar_button_label(*action),
            1,
            accent,
            [238, 244, 250, 255],
        );
    }

    if !overlay.pending_downloads.is_empty() {
        draw_text(
            frame,
            frame_width,
            frame_height,
            left,
            y.saturating_sub(16),
            "PENDING",
            1,
            [162, 173, 189, 255],
        );
        for pending in &overlay.pending_downloads {
            let rect = DesktopUiRect {
                x: left,
                y,
                width,
                height: 36,
            };
            fill_rounded_rect(
                frame,
                frame_width,
                frame_height,
                rect,
                12,
                [255, 255, 255, 10],
            );
            fill_rounded_rect(
                frame,
                frame_width,
                frame_height,
                DesktopUiRect {
                    x: rect.x + 8,
                    y: rect.y + 8,
                    width: 20,
                    height: 20,
                },
                7,
                tint([111, 209, 198, 255], 24),
            );
            draw_symbol(
                frame,
                frame_width,
                frame_height,
                DesktopUiRect {
                    x: rect.x + 12,
                    y: rect.y + 10,
                    width: 12,
                    height: 12,
                },
                DesktopSymbol::Download,
                [111, 209, 198, 255],
            );
            draw_text(
                frame,
                frame_width,
                frame_height,
                left + 36,
                y + 8,
                &normalize_text(&pending.label, 24),
                1,
                [235, 240, 246, 255],
            );
            draw_text(
                frame,
                frame_width,
                frame_height,
                left + 36,
                y + 20,
                &fit_text_to_width(&pending.source_uri, 1, 1, width.saturating_sub(48)).0,
                1,
                [144, 156, 172, 255],
            );
            y = y.saturating_add(44);
        }
    }

    if overlay.pending_downloads.is_empty() && overlay.download_tasks.is_empty() {
        let rect = DesktopUiRect {
            x: left,
            y,
            width,
            height: 58,
        };
        fill_rounded_rect(
            frame,
            frame_width,
            frame_height,
            rect,
            14,
            [255, 255, 255, 8],
        );
        draw_centered_text(
            frame,
            frame_width,
            frame_height,
            rect,
            "NO DOWNLOAD TASKS YET",
            1,
            [170, 182, 195, 255],
        );
        y = y.saturating_add(70);
    }

    if !overlay.download_tasks.is_empty() {
        draw_text(
            frame,
            frame_width,
            frame_height,
            left,
            y.saturating_sub(16),
            "TASKS",
            1,
            [162, 173, 189, 255],
        );
    }

    for task in &overlay.download_tasks {
        let card_height =
            if task.primary_action_label.is_some() || task.export_action_label.is_some() {
                132
            } else {
                106
            };
        let rect = DesktopUiRect {
            x: left,
            y,
            width,
            height: card_height,
        };
        let hovered = overlay
            .cursor_position
            .map(|(x, y)| rect.contains(x, y))
            .unwrap_or(false);
        let accent = task_status_color(task);
        let symbol = task_status_symbol(task);
        let text_x = rect.x + 46;
        let status_badge_width = measure_text(&normalize_text(&task.status, 18), 1)
            .saturating_add(34)
            .max(88);
        fill_rounded_rect(
            frame,
            frame_width,
            frame_height,
            rect,
            16,
            if hovered {
                tint(accent, 18)
            } else {
                [255, 255, 255, 10]
            },
        );
        stroke_rounded_rect(
            frame,
            frame_width,
            frame_height,
            rect,
            16,
            if hovered {
                tint(accent, 52)
            } else {
                [255, 255, 255, 22]
            },
            1,
        );
        fill_rounded_rect(
            frame,
            frame_width,
            frame_height,
            DesktopUiRect {
                x: rect.x + 8,
                y: rect.y + 10,
                width: 3,
                height: rect.height.saturating_sub(20),
            },
            2,
            accent,
        );
        let icon_rect = DesktopUiRect {
            x: rect.x + 16,
            y: rect.y + 14,
            width: 20,
            height: 20,
        };
        fill_rounded_rect(
            frame,
            frame_width,
            frame_height,
            icon_rect,
            7,
            tint(accent, 24),
        );
        draw_symbol(
            frame,
            frame_width,
            frame_height,
            DesktopUiRect {
                x: icon_rect.x + 4,
                y: icon_rect.y + 4,
                width: 12,
                height: 12,
            },
            symbol,
            accent,
        );
        draw_text(
            frame,
            frame_width,
            frame_height,
            text_x,
            rect.y + 12,
            &normalize_text(&task.label, 24),
            1,
            [255, 255, 255, 255],
        );
        draw_badge_with_symbol(
            frame,
            frame_width,
            frame_height,
            DesktopUiRect {
                x: rect
                    .x
                    .saturating_add(rect.width.saturating_sub(status_badge_width + 12)),
                y: rect.y + 10,
                width: status_badge_width,
                height: 18,
            },
            symbol,
            &task.status,
            tint(accent, 18),
            accent,
            [232, 238, 244, 255],
        );
        draw_text(
            frame,
            frame_width,
            frame_height,
            text_x,
            rect.y + 34,
            &normalize_text(&task.progress_summary, 34),
            1,
            [170, 182, 195, 255],
        );
        if let Some(ratio) = task.export_progress.or(task.progress_ratio) {
            let track_rect = DesktopUiRect {
                x: text_x,
                y: rect.y + 52,
                width: rect
                    .width
                    .saturating_sub(text_x.saturating_sub(rect.x) + 16),
                height: 4,
            };
            fill_rounded_rect(
                frame,
                frame_width,
                frame_height,
                track_rect,
                2,
                [255, 255, 255, 18],
            );
            fill_rounded_rect(
                frame,
                frame_width,
                frame_height,
                DesktopUiRect {
                    x: track_rect.x,
                    y: track_rect.y,
                    width: (ratio.clamp(0.0, 1.0) * track_rect.width as f32).round() as u32,
                    height: track_rect.height,
                },
                2,
                accent,
            );
        }
        if task.is_exporting {
            draw_text(
                frame,
                frame_width,
                frame_height,
                text_x,
                rect.y + 64,
                &normalize_text(
                    &format!(
                        "EXPORT {:.0}",
                        task.export_progress.unwrap_or(0.0).clamp(0.0, 1.0) * 100.0
                    ),
                    22,
                ),
                1,
                accent,
            );
        } else if let Some(path) = task.completed_path.as_deref() {
            draw_text(
                frame,
                frame_width,
                frame_height,
                text_x,
                rect.y + 64,
                &fit_text_to_width(path, 1, 1, rect.width.saturating_sub(62)).0,
                1,
                accent,
            );
        } else if let Some(error) = task.error_message.as_deref() {
            draw_text(
                frame,
                frame_width,
                frame_height,
                text_x,
                rect.y + 64,
                &fit_text_to_width(error, 1, 1, rect.width.saturating_sub(62)).0,
                1,
                accent,
            );
        }
        let action_y = rect.y + rect.height.saturating_sub(34);
        let action_width = (width.saturating_sub(20)) / 3;
        if let Some(primary_label) = task.primary_action_label.as_deref() {
            draw_action_button(
                frame,
                frame_width,
                frame_height,
                DesktopUiRect {
                    x: rect.x + 10,
                    y: action_y,
                    width: action_width,
                    height: 24,
                },
                Some(DesktopSymbol::Download),
                primary_label,
                hovered_action == Some(ControlAction::DownloadPrimaryAction(task.task_id)),
                action_accent(ControlAction::DownloadPrimaryAction(task.task_id)),
            );
        }
        if let Some(export_label) = task.export_action_label.as_deref() {
            draw_action_button(
                frame,
                frame_width,
                frame_height,
                DesktopUiRect {
                    x: rect.x + 10 + action_width + 10,
                    y: action_y,
                    width: action_width,
                    height: 24,
                },
                Some(DesktopSymbol::Export),
                export_label,
                hovered_action == Some(ControlAction::DownloadExport(task.task_id)),
                action_accent(ControlAction::DownloadExport(task.task_id)),
            );
        }
        draw_action_button(
            frame,
            frame_width,
            frame_height,
            DesktopUiRect {
                x: rect.x + 10 + (action_width + 10) * 2,
                y: action_y,
                width: action_width,
                height: 24,
            },
            Some(DesktopSymbol::Remove),
            "REMOVE",
            hovered_action == Some(ControlAction::DownloadRemove(task.task_id)),
            action_accent(ControlAction::DownloadRemove(task.task_id)),
        );
        y = y.saturating_add(card_height + 14);
    }

    if let Some(message) = overlay.download_message.as_deref() {
        let rect = DesktopUiRect {
            x: left,
            y: frame_height.saturating_sub(46),
            width,
            height: 28,
        };
        fill_rounded_rect(
            frame,
            frame_width,
            frame_height,
            rect,
            10,
            if message.contains("FAILED") || message.contains("ERROR") {
                [240, 128, 128, 22]
            } else {
                [255, 255, 255, 12]
            },
        );
        draw_centered_text(
            frame,
            frame_width,
            frame_height,
            rect,
            &normalize_text(message, 42),
            1,
            if message.contains("FAILED") || message.contains("ERROR") {
                [240, 162, 162, 255]
            } else {
                [218, 224, 232, 255]
            },
        );
    }
}

fn draw_action_button(
    frame: &mut [u8],
    frame_width: u32,
    frame_height: u32,
    rect: DesktopUiRect,
    symbol: Option<DesktopSymbol>,
    label: &str,
    hovered: bool,
    accent: [u8; 4],
) {
    fill_rounded_rect(
        frame,
        frame_width,
        frame_height,
        rect,
        10,
        if hovered {
            tint(accent, 22)
        } else {
            [255, 255, 255, 12]
        },
    );
    stroke_rounded_rect(
        frame,
        frame_width,
        frame_height,
        rect,
        10,
        if hovered {
            tint(accent, 70)
        } else {
            tint(accent, 44)
        },
        1,
    );
    draw_centered_symbol_label_tones(
        frame,
        frame_width,
        frame_height,
        rect,
        symbol,
        label,
        1,
        accent,
        if hovered {
            [255, 255, 255, 255]
        } else {
            [235, 240, 246, 255]
        },
    );
}

fn draw_host_message(
    frame: &mut [u8],
    frame_width: u32,
    frame_height: u32,
    rect: DesktopUiRect,
    message: &str,
) {
    fill_rect(frame, frame_width, frame_height, rect, [8, 12, 18, 220]);
    stroke_rect(
        frame,
        frame_width,
        frame_height,
        rect,
        [244, 184, 96, 120],
        1,
    );
    draw_text(
        frame,
        frame_width,
        frame_height,
        rect.x + 16,
        rect.y + 16,
        if message.contains("FAILED") {
            "OPEN ERROR"
        } else {
            "LOADING"
        },
        1,
        [244, 184, 96, 255],
    );
    draw_centered_text(
        frame,
        frame_width,
        frame_height,
        DesktopUiRect {
            x: rect.x + 12,
            y: rect.y + 34,
            width: rect.width.saturating_sub(24),
            height: 34,
        },
        &normalize_text(message, 28),
        1,
        [244, 246, 248, 255],
    );
}

fn draw_stage_toolbar(
    frame: &mut [u8],
    frame_width: u32,
    frame_height: u32,
    overlay: &DesktopOverlayViewModel,
    layout: &SidebarLayout,
    opacity: f32,
) {
    let toolbar = layout.stage_toolbar_rect;
    let protocol = overlay_source_protocol(overlay);
    let protocol_color = protocol_accent(protocol);
    let queue_label = format!(
        "{} SOURCE{}",
        overlay.playlist_items.len(),
        if overlay.playlist_items.len() == 1 {
            ""
        } else {
            "S"
        }
    );
    fill_rounded_rect(
        frame,
        frame_width,
        frame_height,
        toolbar,
        16,
        scale_alpha([12, 16, 22, 208], opacity),
    );
    stroke_rounded_rect(
        frame,
        frame_width,
        frame_height,
        toolbar,
        16,
        scale_alpha([255, 255, 255, 22], opacity),
        1,
    );
    let protocol_chip = DesktopUiRect {
        x: toolbar.x + 12,
        y: toolbar.y + 8,
        width: 32,
        height: 32,
    };
    fill_rounded_rect(
        frame,
        frame_width,
        frame_height,
        protocol_chip,
        10,
        scale_alpha(tint(protocol_color, 28), opacity),
    );
    stroke_rounded_rect(
        frame,
        frame_width,
        frame_height,
        protocol_chip,
        10,
        scale_alpha(tint(protocol_color, 70), opacity),
        1,
    );
    draw_symbol(
        frame,
        frame_width,
        frame_height,
        DesktopUiRect {
            x: protocol_chip.x + 7,
            y: protocol_chip.y + 7,
            width: 18,
            height: 18,
        },
        source_protocol_symbol(protocol),
        scale_alpha(protocol_color, opacity),
    );

    let queue_width = measure_text(&normalize_text(&queue_label, 18), 1)
        .saturating_add(34)
        .max(88);
    let state_width = measure_text(&normalize_text(&overlay.playback_state_label, 14), 1)
        .saturating_add(18)
        .max(64);
    let chips_gap = 8;
    let queue_rect = DesktopUiRect {
        x: toolbar.x.saturating_add(
            toolbar
                .width
                .saturating_sub(state_width + queue_width + chips_gap + 16),
        ),
        y: toolbar.y + 14,
        width: queue_width,
        height: 20,
    };
    let state_rect = DesktopUiRect {
        x: queue_rect.x + queue_rect.width + chips_gap,
        y: toolbar.y + 14,
        width: state_width,
        height: 20,
    };
    let title_x = protocol_chip.x + protocol_chip.width + 12;
    let title_width = queue_rect.x.saturating_sub(title_x).saturating_sub(10);

    draw_text(
        frame,
        frame_width,
        frame_height,
        title_x,
        toolbar.y + 10,
        &fit_text_to_width(&overlay.source_label, 1, 1, title_width).0,
        1,
        scale_alpha([255, 255, 255, 255], opacity),
    );
    draw_text(
        frame,
        frame_width,
        frame_height,
        title_x,
        toolbar.y + 28,
        &fit_text_to_width(&overlay.subtitle, 1, 1, title_width).0,
        1,
        scale_alpha([158, 168, 182, 255], opacity),
    );
    draw_badge_with_symbol(
        frame,
        frame_width,
        frame_height,
        queue_rect,
        DesktopSymbol::Playlist,
        &queue_label,
        scale_alpha([255, 255, 255, 12], opacity),
        scale_alpha([188, 199, 214, 255], opacity),
        scale_alpha([214, 221, 230, 255], opacity),
    );
    draw_badge(
        frame,
        frame_width,
        frame_height,
        state_rect,
        &overlay.playback_state_label,
        scale_alpha([244, 184, 96, 34], opacity),
        scale_alpha([244, 184, 96, 255], opacity),
    );
}

fn hovered_action_for_layout(
    layout: &SidebarLayout,
    overlay: &DesktopOverlayViewModel,
) -> Option<ControlAction> {
    let (cursor_x, cursor_y) = overlay.cursor_position?;
    layout
        .buttons
        .iter()
        .find(|button| button.rect.contains(cursor_x, cursor_y))
        .map(|button| button.action)
}

fn control_action_symbol(action: ControlAction, is_playing: bool) -> Option<DesktopSymbol> {
    match action {
        ControlAction::SeekStart => Some(DesktopSymbol::SeekStart),
        ControlAction::SeekBack => Some(DesktopSymbol::SeekBack),
        ControlAction::TogglePause => Some(if is_playing {
            DesktopSymbol::Pause
        } else {
            DesktopSymbol::Play
        }),
        ControlAction::Stop => Some(DesktopSymbol::Stop),
        ControlAction::SeekForward => Some(DesktopSymbol::SeekForward),
        ControlAction::SeekEnd => Some(DesktopSymbol::SeekEnd),
        _ => None,
    }
}

fn action_symbol(action: ControlAction, is_playing: bool) -> Option<DesktopSymbol> {
    control_action_symbol(action, is_playing).or(match action {
        ControlAction::OpenLocalFile => Some(DesktopSymbol::FolderOpen),
        ControlAction::OpenHlsDemo => Some(DesktopSymbol::Stream),
        ControlAction::OpenDashDemo => Some(DesktopSymbol::DashGrid),
        ControlAction::SelectSidebarTab(DesktopSidebarTab::Playlist) => {
            Some(DesktopSymbol::Playlist)
        }
        ControlAction::SelectSidebarTab(DesktopSidebarTab::Downloads) => {
            Some(DesktopSymbol::Download)
        }
        ControlAction::CreateDownloadCurrentSource => Some(DesktopSymbol::LocalLibrary),
        ControlAction::CreateDownloadHlsDemo => Some(DesktopSymbol::Stream),
        ControlAction::CreateDownloadDashDemo => Some(DesktopSymbol::DashGrid),
        ControlAction::DownloadPrimaryAction(_) => Some(DesktopSymbol::Download),
        ControlAction::DownloadExport(_) => Some(DesktopSymbol::Export),
        ControlAction::DownloadRemove(_) => Some(DesktopSymbol::Remove),
        _ => None,
    })
}

fn overlay_source_protocol(overlay: &DesktopOverlayViewModel) -> &'static str {
    let normalized = normalize_text(&overlay.subtitle, overlay.subtitle.chars().count().max(8));
    if normalized.starts_with("HLS") {
        "HLS"
    } else if normalized.starts_with("DASH") {
        "DASH"
    } else if normalized.starts_with("LOCAL") || normalized.starts_with("FILE") {
        "LOCAL"
    } else {
        "SOURCE"
    }
}

fn source_protocol_symbol(protocol: &str) -> DesktopSymbol {
    match protocol {
        "HLS" => DesktopSymbol::Stream,
        "DASH" => DesktopSymbol::DashGrid,
        "LOCAL" => DesktopSymbol::LocalLibrary,
        _ => DesktopSymbol::Waveform,
    }
}

fn protocol_accent(protocol: &str) -> [u8; 4] {
    match protocol {
        "HLS" => [111, 209, 198, 255],
        "DASH" => [142, 171, 255, 255],
        "LOCAL" => [244, 184, 96, 255],
        _ => [188, 199, 214, 255],
    }
}

fn action_accent(action: ControlAction) -> [u8; 4] {
    match action {
        ControlAction::OpenLocalFile | ControlAction::CreateDownloadCurrentSource => {
            protocol_accent("LOCAL")
        }
        ControlAction::OpenHlsDemo | ControlAction::CreateDownloadHlsDemo => protocol_accent("HLS"),
        ControlAction::OpenDashDemo | ControlAction::CreateDownloadDashDemo => {
            protocol_accent("DASH")
        }
        ControlAction::SelectSidebarTab(DesktopSidebarTab::Playlist)
        | ControlAction::FocusPlaylistItem(_) => [244, 184, 96, 255],
        ControlAction::SelectSidebarTab(DesktopSidebarTab::Downloads)
        | ControlAction::DownloadPrimaryAction(_) => [130, 208, 166, 255],
        ControlAction::DownloadExport(_) => [244, 184, 96, 255],
        ControlAction::DownloadRemove(_) => [240, 128, 128, 255],
        _ => [188, 199, 214, 255],
    }
}

fn task_status_color(task: &crate::desktop_ui::DesktopDownloadTaskViewData) -> [u8; 4] {
    if task.error_message.is_some() || task.status == "FAILED" {
        [240, 128, 128, 255]
    } else if task.is_exporting {
        [244, 184, 96, 255]
    } else if task.completed_path.is_some() || task.status == "COMPLETED" {
        [130, 208, 166, 255]
    } else if task.status == "PAUSED" {
        [177, 186, 198, 255]
    } else {
        [111, 209, 198, 255]
    }
}

fn task_status_symbol(task: &crate::desktop_ui::DesktopDownloadTaskViewData) -> DesktopSymbol {
    if task.error_message.is_some() || task.status == "FAILED" {
        DesktopSymbol::AlertTriangle
    } else if task.is_exporting {
        DesktopSymbol::Export
    } else if task.completed_path.is_some() || task.status == "COMPLETED" {
        DesktopSymbol::CheckCircle
    } else {
        DesktopSymbol::Download
    }
}

fn tint(color: [u8; 4], alpha: u8) -> [u8; 4] {
    [color[0], color[1], color[2], alpha]
}

fn draw_badge(
    frame: &mut [u8],
    frame_width: u32,
    frame_height: u32,
    rect: DesktopUiRect,
    label: &str,
    fill: [u8; 4],
    text: [u8; 4],
) {
    fill_rounded_rect(
        frame,
        frame_width,
        frame_height,
        rect,
        rect.height / 2,
        fill,
    );
    draw_centered_text(
        frame,
        frame_width,
        frame_height,
        rect,
        &normalize_text(label, 18),
        1,
        text,
    );
}

fn draw_badge_with_symbol(
    frame: &mut [u8],
    frame_width: u32,
    frame_height: u32,
    rect: DesktopUiRect,
    symbol: DesktopSymbol,
    label: &str,
    fill: [u8; 4],
    symbol_color: [u8; 4],
    text: [u8; 4],
) {
    fill_rounded_rect(
        frame,
        frame_width,
        frame_height,
        rect,
        rect.height / 2,
        fill,
    );
    draw_centered_symbol_label_tones(
        frame,
        frame_width,
        frame_height,
        rect,
        Some(symbol),
        label,
        1,
        symbol_color,
        text,
    );
}

fn draw_centered_symbol_label_tones(
    frame: &mut [u8],
    frame_width: u32,
    frame_height: u32,
    rect: DesktopUiRect,
    symbol: Option<DesktopSymbol>,
    label: &str,
    scale: u32,
    symbol_color: [u8; 4],
    text_color: [u8; 4],
) {
    let gap = 6;
    let icon_size = rect.height.saturating_sub(14).clamp(10, 16);
    let label_max_width = rect.width.saturating_sub(icon_size + gap + 16);
    let (label, label_scale) = fit_text_to_width(label, scale, scale, label_max_width.max(20));
    let text_width = measure_text(&label, label_scale);
    let total_width = text_width
        + symbol
            .map(|_| icon_size.saturating_add(gap))
            .unwrap_or_default();
    let start_x = rect
        .x
        .saturating_add(rect.width.saturating_sub(total_width) / 2);

    if let Some(symbol) = symbol {
        draw_symbol(
            frame,
            frame_width,
            frame_height,
            DesktopUiRect {
                x: start_x,
                y: rect
                    .y
                    .saturating_add(rect.height.saturating_sub(icon_size) / 2),
                width: icon_size,
                height: icon_size,
            },
            symbol,
            symbol_color,
        );
    }
    let text_x = start_x.saturating_add(
        symbol
            .map(|_| icon_size.saturating_add(gap))
            .unwrap_or_default(),
    );
    draw_text(
        frame,
        frame_width,
        frame_height,
        text_x,
        rect.y
            .saturating_add(rect.height.saturating_sub(7 * label_scale) / 2),
        &label,
        label_scale,
        text_color,
    );
}

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

fn ratio_for_progress_x(progress_rect: DesktopUiRect, cursor_x: f64) -> f64 {
    if progress_rect.width == 0 {
        return 0.0;
    }
    ((cursor_x - f64::from(progress_rect.x)) / f64::from(progress_rect.width)).clamp(0.0, 1.0)
}

fn clamp_cursor(value: f64, max: u32) -> u32 {
    value.round().clamp(0.0, f64::from(max.saturating_sub(1))) as u32
}

fn stage_controls_interactive(overlay: &DesktopOverlayViewModel) -> bool {
    overlay.controls_opacity > 0.2 || overlay.host_message.is_some()
}

fn control_button_label(action: ControlAction, _play_pause_label: &'static str) -> &'static str {
    match action {
        ControlAction::SetRate(rate) if (rate - 0.5).abs() < 0.05 => "0.5X",
        ControlAction::SetRate(rate) if (rate - 1.0).abs() < 0.05 => "1X",
        ControlAction::SetRate(rate) if (rate - 1.5).abs() < 0.05 => "1.5X",
        ControlAction::SetRate(rate) if (rate - 2.0).abs() < 0.05 => "2X",
        ControlAction::SetRate(rate) if (rate - 3.0).abs() < 0.05 => "3X",
        _ => "",
    }
}

fn sidebar_button_label(action: ControlAction) -> &'static str {
    match action {
        ControlAction::OpenLocalFile => "OPEN",
        ControlAction::OpenHlsDemo => "HLS DEMO",
        ControlAction::OpenDashDemo => "DASH DEMO",
        ControlAction::SelectSidebarTab(DesktopSidebarTab::Playlist) => "PLAYLIST",
        ControlAction::SelectSidebarTab(DesktopSidebarTab::Downloads) => "DOWNLOADS",
        ControlAction::CreateDownloadCurrentSource => "CURRENT",
        ControlAction::CreateDownloadHlsDemo => "HLS",
        ControlAction::CreateDownloadDashDemo => "DASH",
        ControlAction::DownloadPrimaryAction(_) => "PRIMARY",
        ControlAction::DownloadExport(_) => "EXPORT",
        ControlAction::DownloadRemove(_) => "REMOVE",
        ControlAction::FocusPlaylistItem(_) => "",
        _ => "",
    }
}

fn normalize_text(text: &str, max_chars: usize) -> String {
    let normalized = text
        .chars()
        .map(|character| match character.to_ascii_uppercase() {
            'A'..='Z'
            | '0'..='9'
            | ' '
            | '.'
            | ':'
            | '/'
            | '['
            | ']'
            | '<'
            | '>'
            | '|'
            | '-'
            | '_' => character.to_ascii_uppercase(),
            _ => ' ',
        })
        .collect::<String>();
    let compact = normalized.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut clipped = compact.chars().take(max_chars).collect::<String>();
    if compact.chars().count() > max_chars && max_chars > 3 {
        clipped.truncate(max_chars.saturating_sub(3));
        clipped.push_str("...");
    }
    clipped
}

fn draw_centered_text(
    frame: &mut [u8],
    frame_width: u32,
    frame_height: u32,
    rect: DesktopUiRect,
    text: &str,
    scale: u32,
    color: [u8; 4],
) {
    let text_width = measure_text(text, scale);
    let text_height = 7 * scale;
    let x = rect
        .x
        .saturating_add(rect.width.saturating_sub(text_width) / 2);
    let y = rect
        .y
        .saturating_add(rect.height.saturating_sub(text_height) / 2);
    draw_text(frame, frame_width, frame_height, x, y, text, scale, color);
}

fn measure_text(text: &str, scale: u32) -> u32 {
    let glyph_width = 5 * scale;
    let spacing = scale;
    let char_count = text.chars().count() as u32;
    char_count
        .saturating_mul(glyph_width.saturating_add(spacing))
        .saturating_sub(spacing.min(glyph_width.saturating_add(spacing)))
}

fn fit_text_to_width(
    text: &str,
    preferred_scale: u32,
    fallback_scale: u32,
    max_width: u32,
) -> (String, u32) {
    let normalized = normalize_text(text, text.chars().count().max(4));
    if measure_text(&normalized, preferred_scale) <= max_width {
        return (normalized, preferred_scale);
    }
    if measure_text(&normalized, fallback_scale) <= max_width {
        return (normalized, fallback_scale);
    }

    let ellipsis = "...";
    let ellipsis_width = measure_text(ellipsis, fallback_scale);
    if ellipsis_width >= max_width {
        return (ellipsis.to_owned(), fallback_scale);
    }

    let mut fitted = normalized;
    while !fitted.is_empty() {
        while fitted.ends_with(' ') {
            fitted.pop();
        }
        let candidate = format!("{fitted}{ellipsis}");
        if measure_text(&candidate, fallback_scale) <= max_width {
            return (candidate, fallback_scale);
        }
        fitted.pop();
    }

    (ellipsis.to_owned(), fallback_scale)
}

fn scale_alpha(color: [u8; 4], opacity: f32) -> [u8; 4] {
    let scaled_alpha = (f32::from(color[3]) * opacity.clamp(0.0, 1.0))
        .round()
        .clamp(0.0, 255.0) as u8;
    [color[0], color[1], color[2], scaled_alpha]
}

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
            for column in 0..5_u32 {
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

fn glyph_rows(character: char) -> Option<[u8; 7]> {
    match character.to_ascii_uppercase() {
        'A' => Some([
            0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ]),
        'B' => Some([
            0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110,
        ]),
        'C' => Some([
            0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110,
        ]),
        'D' => Some([
            0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110,
        ]),
        'E' => Some([
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111,
        ]),
        'F' => Some([
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000,
        ]),
        'G' => Some([
            0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01111,
        ]),
        'H' => Some([
            0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ]),
        'I' => Some([
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b11111,
        ]),
        'J' => Some([
            0b11111, 0b00010, 0b00010, 0b00010, 0b10010, 0b10010, 0b01100,
        ]),
        'K' => Some([
            0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001,
        ]),
        'L' => Some([
            0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111,
        ]),
        'M' => Some([
            0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001,
        ]),
        'N' => Some([
            0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001,
        ]),
        'O' => Some([
            0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ]),
        'P' => Some([
            0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000,
        ]),
        'Q' => Some([
            0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101,
        ]),
        'R' => Some([
            0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001,
        ]),
        'S' => Some([
            0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110,
        ]),
        'T' => Some([
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
        ]),
        'U' => Some([
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ]),
        'V' => Some([
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100,
        ]),
        'W' => Some([
            0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b10101, 0b01010,
        ]),
        'X' => Some([
            0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b01010, 0b10001,
        ]),
        'Y' => Some([
            0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
        ]),
        'Z' => Some([
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111,
        ]),
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
        '|' => Some([
            0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
        ]),
        '-' => Some([
            0b00000, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000, 0b00000,
        ]),
        '_' => Some([
            0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b11111,
        ]),
        ' ' => Some([
            0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000,
        ]),
        _ => None,
    }
}

fn fill_rounded_rect(
    frame: &mut [u8],
    frame_width: u32,
    frame_height: u32,
    rect: DesktopUiRect,
    radius: u32,
    color: [u8; 4],
) {
    if rect.width == 0 || rect.height == 0 {
        return;
    }
    let radius = radius.min(rect.width / 2).min(rect.height / 2);
    if radius == 0 {
        fill_rect(frame, frame_width, frame_height, rect, color);
        return;
    }
    let x_end = rect.x.saturating_add(rect.width).min(frame_width);
    let y_end = rect.y.saturating_add(rect.height).min(frame_height);
    let radius_i32 = radius as i32;
    let radius_squared = radius_i32 * radius_i32;
    let right = rect.x.saturating_add(rect.width).saturating_sub(1);
    let bottom = rect.y.saturating_add(rect.height).saturating_sub(1);

    for y in rect.y.min(frame_height)..y_end {
        for x in rect.x.min(frame_width)..x_end {
            let within_horizontal =
                x >= rect.x.saturating_add(radius) && x <= right.saturating_sub(radius);
            let within_vertical =
                y >= rect.y.saturating_add(radius) && y <= bottom.saturating_sub(radius);
            let inside = if within_horizontal || within_vertical {
                true
            } else {
                let corner_center_x = if x < rect.x.saturating_add(radius) {
                    rect.x.saturating_add(radius)
                } else {
                    right.saturating_sub(radius)
                } as i32;
                let corner_center_y = if y < rect.y.saturating_add(radius) {
                    rect.y.saturating_add(radius)
                } else {
                    bottom.saturating_sub(radius)
                } as i32;
                let dx = x as i32 - corner_center_x;
                let dy = y as i32 - corner_center_y;
                dx * dx + dy * dy <= radius_squared
            };
            if inside {
                blend_pixel(frame, frame_width, frame_height, x, y, color);
            }
        }
    }
}

fn fill_circle(
    frame: &mut [u8],
    frame_width: u32,
    frame_height: u32,
    center_x: u32,
    center_y: u32,
    radius: u32,
    color: [u8; 4],
) {
    let radius_i32 = radius as i32;
    let center_x_i32 = center_x as i32;
    let center_y_i32 = center_y as i32;
    let radius_squared = radius_i32 * radius_i32;

    for y in -radius_i32..=radius_i32 {
        for x in -radius_i32..=radius_i32 {
            if x * x + y * y > radius_squared {
                continue;
            }
            let pixel_x = center_x_i32 + x;
            let pixel_y = center_y_i32 + y;
            if pixel_x < 0 || pixel_y < 0 {
                continue;
            }
            blend_pixel(
                frame,
                frame_width,
                frame_height,
                pixel_x as u32,
                pixel_y as u32,
                color,
            );
        }
    }
}

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

fn fill_vertical_gradient(
    frame: &mut [u8],
    frame_width: u32,
    frame_height: u32,
    rect: DesktopUiRect,
    top_color: [u8; 4],
    bottom_color: [u8; 4],
) {
    let y_end = rect.y.saturating_add(rect.height).min(frame_height);
    for y in rect.y.min(frame_height)..y_end {
        let ratio = if rect.height <= 1 {
            0.0
        } else {
            (y.saturating_sub(rect.y)) as f32 / rect.height.saturating_sub(1) as f32
        };
        let color = [
            lerp_channel(top_color[0], bottom_color[0], ratio),
            lerp_channel(top_color[1], bottom_color[1], ratio),
            lerp_channel(top_color[2], bottom_color[2], ratio),
            lerp_channel(top_color[3], bottom_color[3], ratio),
        ];
        fill_rect(
            frame,
            frame_width,
            frame_height,
            DesktopUiRect {
                x: rect.x,
                y,
                width: rect.width,
                height: 1,
            },
            color,
        );
    }
}

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

fn stroke_rounded_rect(
    frame: &mut [u8],
    frame_width: u32,
    frame_height: u32,
    rect: DesktopUiRect,
    radius: u32,
    color: [u8; 4],
    thickness: u32,
) {
    if rect.width == 0 || rect.height == 0 {
        return;
    }
    let inner = DesktopUiRect {
        x: rect.x.saturating_add(thickness),
        y: rect.y.saturating_add(thickness),
        width: rect.width.saturating_sub(thickness.saturating_mul(2)),
        height: rect.height.saturating_sub(thickness.saturating_mul(2)),
    };
    let x_end = rect.x.saturating_add(rect.width).min(frame_width);
    let y_end = rect.y.saturating_add(rect.height).min(frame_height);
    for y in rect.y.min(frame_height)..y_end {
        for x in rect.x.min(frame_width)..x_end {
            let outer = rounded_rect_contains(rect, radius, x, y);
            let inner_contains = inner.width > 0
                && inner.height > 0
                && rounded_rect_contains(inner, radius.saturating_sub(thickness), x, y);
            if outer && !inner_contains {
                blend_pixel(frame, frame_width, frame_height, x, y, color);
            }
        }
    }
}

fn lerp_channel(start: u8, end: u8, ratio: f32) -> u8 {
    (f32::from(start) + (f32::from(end) - f32::from(start)) * ratio.clamp(0.0, 1.0))
        .round()
        .clamp(0.0, 255.0) as u8
}

fn rounded_rect_contains(rect: DesktopUiRect, radius: u32, x: u32, y: u32) -> bool {
    if rect.width == 0 || rect.height == 0 {
        return false;
    }
    let radius = radius.min(rect.width / 2).min(rect.height / 2);
    if radius == 0 {
        return rect.contains(x, y);
    }
    let right = rect.x.saturating_add(rect.width).saturating_sub(1);
    let bottom = rect.y.saturating_add(rect.height).saturating_sub(1);
    let within_horizontal = x >= rect.x.saturating_add(radius) && x <= right.saturating_sub(radius);
    let within_vertical = y >= rect.y.saturating_add(radius) && y <= bottom.saturating_sub(radius);
    if within_horizontal || within_vertical {
        return rect.contains(x, y);
    }
    let corner_x = if x < rect.x.saturating_add(radius) {
        rect.x.saturating_add(radius)
    } else {
        right.saturating_sub(radius)
    } as i32;
    let corner_y = if y < rect.y.saturating_add(radius) {
        rect.y.saturating_add(radius)
    } else {
        bottom.saturating_sub(radius)
    } as i32;
    let dx = x as i32 - corner_x;
    let dy = y as i32 - corner_y;
    dx * dx + dy * dy <= (radius as i32 * radius as i32)
}

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

#[cfg(test)]
mod tests {
    use super::{
        ButtonStyle, fit_text_to_width, overlay_action_at, overlay_layout, playback_stage_rect,
        stage_and_sidebar_rects,
    };
    use crate::desktop_ui::{
        ControlAction, DesktopOverlayViewModel, DesktopPlaylistItemViewData, DesktopUiRect,
    };
    use player_core::{MediaSourceKind, MediaSourceProtocol};
    use player_runtime::{
        PlaybackProgress, PlayerMediaInfo, PlayerResilienceMetrics, PlayerSnapshot,
        PlayerTimelineKind, PlayerTimelineSnapshot, PresentationState,
    };
    use std::time::Duration;

    #[test]
    fn progress_track_stays_below_transport_row() {
        let overlay = test_overlay_view_model();
        let layout = overlay_layout(1365, 967, &overlay).expect("overlay layout");
        let transport_bottom = layout
            .buttons
            .iter()
            .filter(|button| {
                matches!(
                    button.style,
                    ButtonStyle::Utility
                        | ButtonStyle::TransportSecondary
                        | ButtonStyle::TransportPrimary
                )
            })
            .map(|button| button.rect.y.saturating_add(button.rect.height))
            .max()
            .expect("transport controls");

        assert!(
            transport_bottom < layout.progress_rect.y,
            "progress track should stay below transport controls: transport_bottom={}, progress_y={}",
            transport_bottom,
            layout.progress_rect.y
        );
    }

    #[test]
    fn compact_stage_keeps_transport_cluster_centered_in_panel() {
        let overlay = test_overlay_view_model();
        let layout = overlay_layout(1024, 720, &overlay).expect("overlay layout");
        let bounds = layout
            .buttons
            .iter()
            .filter(|button| {
                matches!(
                    button.style,
                    ButtonStyle::Utility
                        | ButtonStyle::TransportSecondary
                        | ButtonStyle::TransportPrimary
                )
            })
            .fold(None::<DesktopUiRect>, |bounds, button| {
                let rect = button.rect;
                Some(match bounds {
                    Some(existing) => {
                        let left = existing.x.min(rect.x);
                        let top = existing.y.min(rect.y);
                        let right = existing
                            .x
                            .saturating_add(existing.width)
                            .max(rect.x.saturating_add(rect.width));
                        let bottom = existing
                            .y
                            .saturating_add(existing.height)
                            .max(rect.y.saturating_add(rect.height));
                        DesktopUiRect {
                            x: left,
                            y: top,
                            width: right.saturating_sub(left),
                            height: bottom.saturating_sub(top),
                        }
                    }
                    None => rect,
                })
            })
            .expect("transport bounds");

        let transport_center = bounds.x.saturating_add(bounds.width / 2);
        let panel_center = layout
            .control_bar_rect
            .x
            .saturating_add(layout.control_bar_rect.width / 2);
        assert!(
            transport_center.abs_diff(panel_center) <= 12,
            "transport cluster should stay centered in the floating panel: transport_center={}, panel_center={}",
            transport_center,
            panel_center
        );
        assert!(
            bounds.x >= layout.control_bar_rect.x.saturating_add(10)
                && bounds.x.saturating_add(bounds.width)
                    <= layout
                        .control_bar_rect
                        .x
                        .saturating_add(layout.control_bar_rect.width)
                        .saturating_sub(10),
            "transport cluster should stay inside the floating panel: left={}, right={}, panel_left={}, panel_right={}",
            bounds.x,
            bounds.x.saturating_add(bounds.width),
            layout.control_bar_rect.x,
            layout
                .control_bar_rect
                .x
                .saturating_add(layout.control_bar_rect.width)
        );
    }

    #[test]
    fn progress_track_scales_with_stage_width() {
        let overlay = test_overlay_view_model();
        let wide = overlay_layout(1365, 967, &overlay).expect("wide layout");
        let compact = overlay_layout(1024, 720, &overlay).expect("compact layout");

        assert!(
            compact.progress_rect.width < wide.progress_rect.width,
            "progress width should shrink with stage width: compact={}, wide={}",
            compact.progress_rect.width,
            wide.progress_rect.width
        );
    }

    #[test]
    fn rate_strip_stays_centered_above_floating_panel() {
        let overlay = test_overlay_view_model();
        let layout = overlay_layout(1365, 967, &overlay).expect("overlay layout");
        let bounds = layout
            .buttons
            .iter()
            .filter(|button| matches!(button.style, ButtonStyle::Rate))
            .fold(None::<DesktopUiRect>, |bounds, button| {
                let rect = button.rect;
                Some(match bounds {
                    Some(existing) => {
                        let left = existing.x.min(rect.x);
                        let top = existing.y.min(rect.y);
                        let right = existing
                            .x
                            .saturating_add(existing.width)
                            .max(rect.x.saturating_add(rect.width));
                        let bottom = existing
                            .y
                            .saturating_add(existing.height)
                            .max(rect.y.saturating_add(rect.height));
                        DesktopUiRect {
                            x: left,
                            y: top,
                            width: right.saturating_sub(left),
                            height: bottom.saturating_sub(top),
                        }
                    }
                    None => rect,
                })
            })
            .expect("rate bounds");
        let strip_center = bounds.x.saturating_add(bounds.width / 2);
        let panel_center = layout
            .control_bar_rect
            .x
            .saturating_add(layout.control_bar_rect.width / 2);
        assert!(
            strip_center.abs_diff(panel_center) <= 6,
            "rate strip should be centered above floating panel: strip_center={}, panel_center={}",
            strip_center,
            panel_center
        );
    }

    #[test]
    fn long_sidebar_label_scales_down_to_fit() {
        let (label, scale) = fit_text_to_width("HLS DEMO", 2, 1, 88 - 12);
        assert_eq!(scale, 1);
        assert!(super::measure_text(&label, scale) <= 76);
    }

    #[test]
    fn hls_demo_hit_target_resolves_to_open_hls_action() {
        let overlay = test_overlay_view_model();
        let snapshot = test_snapshot();
        let layout = overlay_layout(1365, 967, &overlay).expect("overlay layout");
        let hls_button = layout
            .buttons
            .iter()
            .find(|button| matches!(button.action, ControlAction::OpenHlsDemo))
            .expect("hls button");
        let center_x = f64::from(hls_button.rect.x + hls_button.rect.width / 2);
        let center_y = f64::from(hls_button.rect.y + hls_button.rect.height / 2);

        let action = overlay_action_at(1365, 967, center_x, center_y, &snapshot, &overlay);
        assert_eq!(action, Some(ControlAction::OpenHlsDemo));
    }

    #[test]
    fn downloads_tab_swaps_playlist_rows_for_download_actions() {
        let playlist_overlay = test_overlay_view_model();
        let playlist_layout =
            overlay_layout(1365, 967, &playlist_overlay).expect("playlist layout");
        assert!(
            playlist_layout
                .buttons
                .iter()
                .any(|button| matches!(button.action, ControlAction::FocusPlaylistItem(0))),
            "playlist tab should expose playlist focus rows"
        );
        assert!(
            !playlist_layout
                .buttons
                .iter()
                .any(|button| matches!(button.action, ControlAction::CreateDownloadCurrentSource)),
            "playlist tab should not expose download create actions in the content area"
        );

        let mut downloads_overlay = test_overlay_view_model();
        downloads_overlay.sidebar_tab = crate::desktop_ui::DesktopSidebarTab::Downloads;
        let downloads_layout =
            overlay_layout(1365, 967, &downloads_overlay).expect("downloads layout");
        assert!(
            downloads_layout
                .buttons
                .iter()
                .any(|button| matches!(button.action, ControlAction::CreateDownloadCurrentSource)),
            "downloads tab should expose download create actions"
        );
        assert!(
            !downloads_layout
                .buttons
                .iter()
                .any(|button| matches!(button.action, ControlAction::FocusPlaylistItem(_))),
            "downloads tab should hide playlist focus rows"
        );
    }

    #[test]
    fn stage_toolbar_stays_inside_video_stage() {
        let overlay = test_overlay_view_model();
        let layout = overlay_layout(1365, 967, &overlay).expect("overlay layout");
        assert!(layout.stage_toolbar_rect.x < layout.sidebar_rect.x);
        assert!(
            layout
                .stage_toolbar_rect
                .x
                .saturating_add(layout.stage_toolbar_rect.width)
                <= layout.sidebar_rect.x
        );
        assert!(layout.stage_toolbar_rect.y < layout.control_bar_rect.y);
    }

    #[test]
    fn stage_rect_reserves_space_for_sidebar_on_wide_surface() {
        let (stage_rect, sidebar_rect) =
            stage_and_sidebar_rects(1365, 967).expect("wide surface should split stage/sidebar");
        assert_eq!(stage_rect.x, 0);
        assert_eq!(stage_rect.width.saturating_add(sidebar_rect.width), 1365);
        assert_eq!(sidebar_rect.x, stage_rect.width);
    }

    #[test]
    fn stage_rect_falls_back_to_full_width_on_compact_surface() {
        assert!(stage_and_sidebar_rects(720, 480).is_none());
        let stage_rect = playback_stage_rect(720, 480);
        assert_eq!(stage_rect.width, 720);
        assert_eq!(stage_rect.height, 480);
    }

    fn test_overlay_view_model() -> DesktopOverlayViewModel {
        DesktopOverlayViewModel {
            source_label: "TEST VIDEO".to_owned(),
            playback_state_label: "Paused".to_owned(),
            subtitle: "LOCAL 960X432".to_owned(),
            controls_opacity: 1.0,
            cursor_position: None,
            sidebar_tab: crate::desktop_ui::DesktopSidebarTab::Playlist,
            playlist_items: vec![DesktopPlaylistItemViewData {
                label: "TEST VIDEO".to_owned(),
                status: "CURRENT".to_owned(),
                is_active: true,
            }],
            pending_downloads: Vec::new(),
            download_tasks: Vec::new(),
            host_message: None,
            download_message: None,
            export_plugin_installed: true,
        }
    }

    fn test_snapshot() -> PlayerSnapshot {
        let progress =
            PlaybackProgress::new(Duration::from_secs(26), Some(Duration::from_secs(600)));
        PlayerSnapshot {
            source_uri: "https://example.invalid/master.m3u8".to_owned(),
            state: PresentationState::Paused,
            has_video_surface: true,
            is_interrupted: false,
            is_buffering: false,
            playback_rate: 1.0,
            progress,
            timeline: PlayerTimelineSnapshot {
                kind: PlayerTimelineKind::Vod,
                is_seekable: true,
                seekable_range: None,
                live_edge: None,
                position: Duration::from_secs(26),
                duration: Some(Duration::from_secs(600)),
            },
            media_info: PlayerMediaInfo {
                source_uri: "https://example.invalid/master.m3u8".to_owned(),
                source_kind: MediaSourceKind::Remote,
                source_protocol: MediaSourceProtocol::Hls,
                duration: Some(Duration::from_secs(600)),
                bit_rate: None,
                audio_streams: 1,
                video_streams: 1,
                best_video: None,
                best_audio: None,
                track_catalog: Default::default(),
                track_selection: Default::default(),
            },
            resilience_metrics: PlayerResilienceMetrics::default(),
        }
    }
}
