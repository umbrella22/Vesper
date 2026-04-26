#![allow(dead_code)]

use std::collections::VecDeque;
use std::ffi::{c_char, c_float, c_void};
use std::sync::Mutex;

use anyhow::{Context, Result};
use player_render_wgpu::RgbaOverlayFrame;
use player_runtime::PlayerSnapshot;
use tracing::info;
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::Window;

use crate::desktop_ui::{ControlAction, DesktopUiLayoutMetrics, DesktopUiViewModel};

pub struct MacosHostOverlay {
    handle: *mut c_void,
    callback_context: Box<MacosHostOverlayCallbackContext>,
}

pub struct MacosBitmapOverlay {
    handle: *mut c_void,
}

struct MacosHostOverlayCallbackContext {
    actions: Mutex<VecDeque<ControlAction>>,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct BasicPlayerMacosOverlayCallbacks {
    on_action: Option<extern "C" fn(*mut c_void, u32, c_float)>,
    context: *mut c_void,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct BasicPlayerMacosOverlayState {
    is_playing: u8,
    has_duration: u8,
    timeline_kind: u32,
    is_seekable: u8,
    controls_visible: u8,
    position_ms: u64,
    duration_ms: u64,
    seekable_start_ms: u64,
    seekable_end_ms: u64,
    playback_rate: c_float,
    bar_height: u32,
    padding: u32,
    gap: u32,
    icon_size: u32,
    rate_width: u32,
    progress_height: u32,
    progress_hit_slop_top: u32,
    progress_hit_slop_bottom: u32,
    time_label_height: u32,
}

const ACTION_SEEK_START: u32 = 0;
const ACTION_SEEK_BACK: u32 = 1;
const ACTION_TOGGLE_PAUSE: u32 = 2;
const ACTION_STOP: u32 = 3;
const ACTION_SEEK_FORWARD: u32 = 4;
const ACTION_SEEK_END: u32 = 5;
const ACTION_SET_RATE: u32 = 6;
const ACTION_SEEK_TO_RATIO: u32 = 7;

impl MacosHostOverlay {
    pub fn attach(window: &Window) -> Result<Self> {
        let ns_view_handle = macos_ns_view_handle(window)?;
        let callback_context = Box::new(MacosHostOverlayCallbackContext {
            actions: Mutex::new(VecDeque::new()),
        });
        let callbacks = BasicPlayerMacosOverlayCallbacks {
            on_action: Some(macos_host_overlay_on_action),
            context: ((&*callback_context as *const MacosHostOverlayCallbackContext)
                as *mut MacosHostOverlayCallbackContext)
                .cast(),
        };
        let mut overlay_handle = std::ptr::null_mut();
        let mut error_message = [0 as c_char; 256];
        let created = unsafe {
            basic_player_macos_overlay_create(
                ns_view_handle as usize,
                callbacks,
                &mut overlay_handle,
                error_message.as_mut_ptr(),
                error_message.len(),
            )
        };
        if !created {
            anyhow::bail!(
                "{}",
                c_string_buffer_to_string(&error_message)
                    .if_empty("failed to create macOS host overlay")
            );
        }

        Ok(Self {
            handle: overlay_handle,
            callback_context,
        })
    }

    pub fn update(
        &self,
        snapshot: &PlayerSnapshot,
        controls_visible: bool,
        layout_metrics: DesktopUiLayoutMetrics,
    ) {
        let view_model = DesktopUiViewModel::from_snapshot(snapshot, controls_visible, None);
        let state = BasicPlayerMacosOverlayState {
            is_playing: view_model.is_playing as u8,
            has_duration: view_model.has_duration as u8,
            timeline_kind: view_model.timeline_kind.as_raw(),
            is_seekable: view_model.is_seekable as u8,
            controls_visible: view_model.controls_visible as u8,
            position_ms: duration_to_millis_u64(view_model.displayed_position),
            duration_ms: view_model
                .duration
                .map(duration_to_millis_u64)
                .unwrap_or_default(),
            seekable_start_ms: view_model
                .seekable_range
                .map(|range| duration_to_millis_u64(range.start))
                .unwrap_or_default(),
            seekable_end_ms: view_model
                .seekable_range
                .map(|range| duration_to_millis_u64(range.end))
                .unwrap_or_default(),
            playback_rate: view_model.playback_rate,
            bar_height: layout_metrics.bar_height,
            padding: layout_metrics.padding,
            gap: layout_metrics.gap,
            icon_size: layout_metrics.icon_size,
            rate_width: layout_metrics.rate_width,
            progress_height: layout_metrics.progress_height,
            progress_hit_slop_top: layout_metrics.progress_hit_slop_top,
            progress_hit_slop_bottom: layout_metrics.progress_hit_slop_bottom,
            time_label_height: layout_metrics.time_label_height,
        };

        unsafe {
            basic_player_macos_overlay_update(self.handle, state);
        }
    }

    pub fn drain_actions(&self) -> Vec<ControlAction> {
        self.callback_context
            .actions
            .lock()
            .map(|mut actions| actions.drain(..).collect())
            .unwrap_or_default()
    }
}

impl MacosBitmapOverlay {
    pub fn attach(window: &Window) -> Result<Self> {
        let ns_view_handle = macos_ns_view_handle(window)?;
        let mut overlay_handle = std::ptr::null_mut();
        let mut error_message = [0 as c_char; 256];
        let created = unsafe {
            basic_player_macos_bitmap_overlay_create(
                ns_view_handle as usize,
                &mut overlay_handle,
                error_message.as_mut_ptr(),
                error_message.len(),
            )
        };
        if !created {
            anyhow::bail!(
                "{}",
                c_string_buffer_to_string(&error_message)
                    .if_empty("failed to create macOS bitmap overlay")
            );
        }

        Ok(Self {
            handle: overlay_handle,
        })
    }

    pub fn update(&self, overlay: &RgbaOverlayFrame) -> Result<()> {
        let mut error_message = [0 as c_char; 256];
        let updated = unsafe {
            basic_player_macos_bitmap_overlay_update(
                self.handle,
                overlay.bytes.as_ptr(),
                overlay.bytes.len(),
                overlay.width,
                overlay.height,
                error_message.as_mut_ptr(),
                error_message.len(),
            )
        };
        if !updated {
            anyhow::bail!(
                "{}",
                c_string_buffer_to_string(&error_message)
                    .if_empty("failed to update macOS bitmap overlay")
            );
        }
        Ok(())
    }

    pub fn clear(&self) {
        unsafe {
            basic_player_macos_bitmap_overlay_clear(self.handle);
        }
    }
}

impl Drop for MacosBitmapOverlay {
    fn drop(&mut self) {
        unsafe {
            basic_player_macos_bitmap_overlay_destroy(self.handle);
        }
    }
}

fn duration_to_millis_u64(duration: std::time::Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
}

impl Drop for MacosHostOverlay {
    fn drop(&mut self) {
        unsafe {
            basic_player_macos_overlay_destroy(self.handle);
        }
    }
}

extern "C" fn macos_host_overlay_on_action(context: *mut c_void, action_kind: u32, rate: c_float) {
    let Some(context) = (unsafe { context.cast::<MacosHostOverlayCallbackContext>().as_ref() })
    else {
        return;
    };

    let Some(action) = map_action(action_kind, rate) else {
        return;
    };

    if let Ok(mut actions) = context.actions.lock() {
        info!(?action, "macOS host overlay action queued");
        actions.push_back(action);
    }
}

fn map_action(action_kind: u32, rate: c_float) -> Option<ControlAction> {
    match action_kind {
        ACTION_SEEK_START => Some(ControlAction::SeekStart),
        ACTION_SEEK_BACK => Some(ControlAction::SeekBack),
        ACTION_TOGGLE_PAUSE => Some(ControlAction::TogglePause),
        ACTION_STOP => Some(ControlAction::Stop),
        ACTION_SEEK_FORWARD => Some(ControlAction::SeekForward),
        ACTION_SEEK_END => Some(ControlAction::SeekEnd),
        ACTION_SET_RATE if rate.is_finite() => Some(ControlAction::SetRate(rate)),
        ACTION_SEEK_TO_RATIO if rate.is_finite() => Some(ControlAction::SeekToRatio(rate)),
        _ => None,
    }
}

fn macos_ns_view_handle(window: &Window) -> Result<*mut c_void> {
    let handle = window
        .window_handle()
        .context("failed to resolve the macOS raw window handle for host overlay")?;
    match handle.as_raw() {
        RawWindowHandle::AppKit(handle) => Ok(handle.ns_view.as_ptr()),
        raw => anyhow::bail!("expected an AppKit window handle on macOS, received {raw:?}"),
    }
}

fn c_string_buffer_to_string(buffer: &[c_char]) -> String {
    unsafe { std::ffi::CStr::from_ptr(buffer.as_ptr()) }
        .to_string_lossy()
        .into_owned()
}

trait EmptyFallback {
    fn if_empty(self, fallback: &str) -> String;
}

impl EmptyFallback for String {
    fn if_empty(self, fallback: &str) -> String {
        if self.is_empty() {
            fallback.to_owned()
        } else {
            self
        }
    }
}

unsafe extern "C" {
    fn basic_player_macos_overlay_create(
        ns_view_handle: usize,
        callbacks: BasicPlayerMacosOverlayCallbacks,
        out_overlay: *mut *mut c_void,
        error_message: *mut c_char,
        error_message_size: usize,
    ) -> bool;

    fn basic_player_macos_overlay_destroy(overlay_handle: *mut c_void);

    fn basic_player_macos_overlay_update(
        overlay_handle: *mut c_void,
        state: BasicPlayerMacosOverlayState,
    );

    fn basic_player_macos_bitmap_overlay_create(
        ns_view_handle: usize,
        out_overlay: *mut *mut c_void,
        error_message: *mut c_char,
        error_message_size: usize,
    ) -> bool;

    fn basic_player_macos_bitmap_overlay_update(
        overlay_handle: *mut c_void,
        bytes: *const u8,
        byte_length: usize,
        width: u32,
        height: u32,
        error_message: *mut c_char,
        error_message_size: usize,
    ) -> bool;

    fn basic_player_macos_bitmap_overlay_clear(overlay_handle: *mut c_void);

    fn basic_player_macos_bitmap_overlay_destroy(overlay_handle: *mut c_void);
}
