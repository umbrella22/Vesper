#[cfg(target_os = "android")]
use std::ffi::c_void;
#[cfg(target_os = "android")]
use std::ptr::NonNull;

use jni::Env;
use jni::objects::{Global, JObject};
#[cfg(target_os = "android")]
use jni::sys::{JNIEnv, jobject};
use player_platform_android::{
    AndroidNativeFramePresenterFrame, AndroidNativeFramePresenterSink,
    AndroidNativeFramePresenterSubmitResult,
};
use player_plugin::DecoderNativeDeviceContext;
use player_runtime::{PlayerError, PlayerErrorCode, PlayerResult};

pub(crate) struct AndroidNativeWindowPresenterSink {
    surface: Global<JObject<'static>>,
    #[cfg(target_os = "android")]
    native_window: NonNull<ANativeWindow>,
    closed: bool,
}

// SAFETY: The sink owns a global Java Surface reference and an ANativeWindow reference.
// ANativeWindow references are explicitly reference-counted and may be retained across
// JNI calls. The current submit path does not dereference or mutate the window from
// background threads; it only preserves the native resource boundary for the session.
unsafe impl Send for AndroidNativeWindowPresenterSink {}

#[cfg(target_os = "android")]
#[repr(C)]
struct ANativeWindow {
    _private: [u8; 0],
}

#[cfg(target_os = "android")]
#[link(name = "android")]
unsafe extern "C" {
    fn ANativeWindow_fromSurface(env: *mut JNIEnv, surface: jobject) -> *mut ANativeWindow;
    fn ANativeWindow_release(window: *mut ANativeWindow);
}

impl AndroidNativeWindowPresenterSink {
    pub(crate) fn from_surface(env: &mut Env<'_>, surface: &JObject<'_>) -> Result<Self, String> {
        if surface.is_null() {
            return Err("Android native-frame presenter received a null Surface".to_owned());
        }
        let global_surface = env
            .new_global_ref(surface)
            .map_err(|error| format!("failed to create global Surface reference: {error}"))?;
        #[cfg(not(target_os = "android"))]
        {
            let _ = global_surface;
            return Err(
                "Android native-frame presenter requires an Android runtime Surface".to_owned(),
            );
        }
        #[cfg(target_os = "android")]
        {
            let native_window =
            // SAFETY: `env.get_raw()` is the active JNI environment for this call and
            // `surface.as_raw()` is the non-null android.view.Surface local reference
            // supplied by the Kotlin host. ANativeWindow_fromSurface returns a retained
            // native window reference or null on failure; ownership is released in close/drop.
            unsafe { ANativeWindow_fromSurface(env.get_raw(), surface.as_raw()) };
            let native_window = NonNull::new(native_window)
                .ok_or_else(|| "ANativeWindow_fromSurface returned null".to_owned())?;
            Ok(Self {
                surface: global_surface,
                native_window,
                closed: false,
            })
        }
    }

    #[cfg(target_os = "android")]
    fn native_window_addr(&self) -> usize {
        self.native_window.as_ptr().cast::<c_void>() as usize
    }

    #[cfg(not(target_os = "android"))]
    fn native_window_addr(&self) -> usize {
        0
    }
}

impl AndroidNativeFramePresenterSink for AndroidNativeWindowPresenterSink {
    fn submit_frame(
        &mut self,
        frame: &AndroidNativeFramePresenterFrame,
    ) -> PlayerResult<AndroidNativeFramePresenterSubmitResult> {
        if self.closed {
            return Err(PlayerError::new(
                PlayerErrorCode::InvalidState,
                "Android native-frame presenter is closed",
            ));
        }
        let _surface_ref = self.surface.as_raw();
        let _native_window = self.native_window_addr();
        Ok(AndroidNativeFramePresenterSubmitResult {
            accepted: true,
            requires_host_release: true,
            message: Some(format!(
                "Android native presenter accepted frame {} for host-timed MediaCodec release-to-surface",
                frame.frame_handle
            )),
        })
    }

    fn decoder_device_context(&self) -> Option<DecoderNativeDeviceContext> {
        #[cfg(not(target_os = "android"))]
        {
            None
        }
        #[cfg(target_os = "android")]
        {
            Some(DecoderNativeDeviceContext::AndroidNativeWindow {
                window_ptr: self.native_window_addr(),
            })
        }
    }

    fn flush(&mut self) -> PlayerResult<()> {
        Ok(())
    }

    fn close(&mut self) -> PlayerResult<()> {
        if !self.closed {
            #[cfg(target_os = "android")]
            {
                // SAFETY: `native_window` was returned retained by ANativeWindow_fromSurface
                // and is released exactly once here or in Drop.
                unsafe { ANativeWindow_release(self.native_window.as_ptr()) };
            }
            self.closed = true;
        }
        Ok(())
    }
}

impl Drop for AndroidNativeWindowPresenterSink {
    fn drop(&mut self) {
        let _ = self.close();
    }
}
