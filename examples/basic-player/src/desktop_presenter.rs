use anyhow::Result;
use player_render_wgpu::RgbaOverlayFrame;
use player_runtime::PlayerSnapshot;
use winit::dpi::PhysicalSize;
use winit::window::Window;

use crate::desktop_overlay_ui::{
    overlay_action_at, render_desktop_overlay, seek_preview_at, seek_preview_for_drag,
};
use crate::desktop_ui::{ControlAction, DesktopOverlayViewModel, SeekPreview};
#[cfg(target_os = "macos")]
use crate::macos_host_overlay::MacosBitmapOverlay;

pub struct DesktopUiPresenter {
    #[cfg(target_os = "macos")]
    native_overlay: Option<MacosBitmapOverlay>,
}

impl DesktopUiPresenter {
    pub fn attach(window: &Window) -> Result<Self> {
        #[cfg(target_os = "macos")]
        {
            Ok(Self {
                native_overlay: Some(MacosBitmapOverlay::attach(window)?),
            })
        }

        #[cfg(not(target_os = "macos"))]
        {
            let _ = window;
            Ok(Self {})
        }
    }

    pub fn sync(
        &self,
        snapshot: &PlayerSnapshot,
        overlay: &DesktopOverlayViewModel,
        window_size: PhysicalSize<u32>,
        window_scale_factor: f64,
        seek_preview: Option<SeekPreview>,
        native_overlay_enabled: bool,
    ) {
        #[cfg(target_os = "macos")]
        {
            let Some(native_overlay) = self.native_overlay.as_ref() else {
                return;
            };
            if !native_overlay_enabled {
                native_overlay.clear();
                return;
            }
            match render_desktop_overlay(
                window_size.width,
                window_size.height,
                window_scale_factor,
                snapshot,
                seek_preview,
                overlay,
            ) {
                Some(frame) => {
                    let _ = native_overlay.update(&frame);
                }
                None => native_overlay.clear(),
            }
        }

        #[cfg(not(target_os = "macos"))]
        {
            let _ = (
                self,
                snapshot,
                overlay,
                window_size,
                window_scale_factor,
                seek_preview,
                native_overlay_enabled,
            );
        }
    }

    pub fn drain_actions(&self) -> Vec<ControlAction> {
        Vec::new()
    }

    pub fn overlay_frame(
        &self,
        window_size: PhysicalSize<u32>,
        window_scale_factor: f64,
        snapshot: &PlayerSnapshot,
        seek_preview: Option<SeekPreview>,
        overlay: &DesktopOverlayViewModel,
    ) -> Option<RgbaOverlayFrame> {
        render_desktop_overlay(
            window_size.width,
            window_size.height,
            window_scale_factor,
            snapshot,
            seek_preview,
            overlay,
        )
    }

    pub fn control_action_at(
        &self,
        window_size: PhysicalSize<u32>,
        window_scale_factor: f64,
        cursor_x: f64,
        cursor_y: f64,
        snapshot: &PlayerSnapshot,
        overlay: &DesktopOverlayViewModel,
    ) -> Option<ControlAction> {
        overlay_action_at(
            window_size.width,
            window_size.height,
            window_scale_factor,
            cursor_x,
            cursor_y,
            snapshot,
            overlay,
        )
    }

    pub fn seek_preview_at(
        &self,
        window_size: PhysicalSize<u32>,
        window_scale_factor: f64,
        cursor_x: f64,
        cursor_y: f64,
        snapshot: &PlayerSnapshot,
        overlay: &DesktopOverlayViewModel,
    ) -> Option<SeekPreview> {
        seek_preview_at(
            window_size.width,
            window_size.height,
            window_scale_factor,
            cursor_x,
            cursor_y,
            snapshot,
            overlay,
        )
    }

    pub fn seek_preview_for_drag(
        &self,
        window_size: PhysicalSize<u32>,
        window_scale_factor: f64,
        cursor_x: f64,
        snapshot: &PlayerSnapshot,
        overlay: &DesktopOverlayViewModel,
    ) -> Option<SeekPreview> {
        seek_preview_for_drag(
            window_size.width,
            window_size.height,
            window_scale_factor,
            cursor_x,
            snapshot,
            overlay,
        )
    }
}
