use anyhow::Result;
use player_render_wgpu::RgbaOverlayFrame;
use player_runtime::PlayerSnapshot;
use winit::dpi::PhysicalSize;
use winit::window::Window;

use crate::desktop_ui::{ControlAction, SeekPreview};
#[cfg(target_os = "macos")]
use crate::desktop_ui::DesktopUiLayoutMetrics;

#[cfg(not(target_os = "macos"))]
use crate::host_ui::{
    control_action_at, render_control_overlay, seek_preview_at, seek_preview_for_drag,
};
#[cfg(target_os = "macos")]
use crate::macos_host_overlay::MacosHostOverlay;

pub enum DesktopUiPresenter {
    #[cfg(target_os = "macos")]
    Macos(MacosHostOverlay),
    #[cfg(not(target_os = "macos"))]
    Software,
}

impl DesktopUiPresenter {
    pub fn attach(window: &Window) -> Result<Self> {
        #[cfg(target_os = "macos")]
        {
            return Ok(Self::Macos(MacosHostOverlay::attach(window)?));
        }

        #[cfg(not(target_os = "macos"))]
        {
            let _ = window;
            Ok(Self::Software)
        }
    }

    pub fn sync(
        &self,
        snapshot: &PlayerSnapshot,
        controls_visible: bool,
        window_size: PhysicalSize<u32>,
    ) {
        #[cfg(target_os = "macos")]
        {
            let Self::Macos(overlay) = self;
            if let Some(layout_metrics) =
                DesktopUiLayoutMetrics::for_surface(window_size.width.max(1), window_size.height.max(1))
            {
                overlay.update(snapshot, controls_visible, layout_metrics);
            }
        }

        #[cfg(not(target_os = "macos"))]
        {
            let _ = (self, snapshot, controls_visible, window_size);
        }
    }

    pub fn drain_actions(&self) -> Vec<ControlAction> {
        #[cfg(target_os = "macos")]
        {
            let Self::Macos(overlay) = self;
            return overlay.drain_actions();
        }

        #[cfg(not(target_os = "macos"))]
        {
            let _ = self;
            Vec::new()
        }
    }

    pub fn overlay_frame(
        &self,
        window_size: PhysicalSize<u32>,
        snapshot: &PlayerSnapshot,
        seek_preview: Option<SeekPreview>,
        controls_visible: bool,
    ) -> Option<RgbaOverlayFrame> {
        #[cfg(not(target_os = "macos"))]
        {
            let _ = self;
            if !controls_visible || window_size.width == 0 || window_size.height == 0 {
                return None;
            }

            return render_control_overlay(
                window_size.width,
                window_size.height,
                snapshot,
                seek_preview,
            );
        }

        #[cfg(target_os = "macos")]
        {
            let _ = (self, window_size, snapshot, seek_preview, controls_visible);
            None
        }
    }

    pub fn control_action_at(
        &self,
        window_size: PhysicalSize<u32>,
        cursor_x: f64,
        cursor_y: f64,
        snapshot: &PlayerSnapshot,
    ) -> Option<ControlAction> {
        #[cfg(not(target_os = "macos"))]
        {
            let _ = self;
            if window_size.width == 0 || window_size.height == 0 {
                return None;
            }

            return control_action_at(
                window_size.width,
                window_size.height,
                cursor_x,
                cursor_y,
                snapshot,
            );
        }

        #[cfg(target_os = "macos")]
        {
            let _ = (self, window_size, cursor_x, cursor_y, snapshot);
            None
        }
    }

    pub fn seek_preview_at(
        &self,
        window_size: PhysicalSize<u32>,
        cursor_x: f64,
        cursor_y: f64,
        snapshot: &PlayerSnapshot,
    ) -> Option<SeekPreview> {
        #[cfg(not(target_os = "macos"))]
        {
            let _ = self;
            if window_size.width == 0 || window_size.height == 0 {
                return None;
            }

            return seek_preview_at(
                window_size.width,
                window_size.height,
                cursor_x,
                cursor_y,
                snapshot,
            );
        }

        #[cfg(target_os = "macos")]
        {
            let _ = (self, window_size, cursor_x, cursor_y, snapshot);
            None
        }
    }

    pub fn seek_preview_for_drag(
        &self,
        window_size: PhysicalSize<u32>,
        cursor_x: f64,
        snapshot: &PlayerSnapshot,
    ) -> Option<SeekPreview> {
        #[cfg(not(target_os = "macos"))]
        {
            let _ = self;
            if window_size.width == 0 || window_size.height == 0 {
                return None;
            }

            return seek_preview_for_drag(window_size.width, window_size.height, cursor_x, snapshot);
        }

        #[cfg(target_os = "macos")]
        {
            let _ = (self, window_size, cursor_x, snapshot);
            None
        }
    }
}