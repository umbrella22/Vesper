//! Native loading and static registration.

mod audio;
mod filter;
mod inspection;
mod library;
mod post_download;
mod source;
mod source_adapter;
mod video;
mod video_adapter;

pub use filter::{NativeVideoFilter, RecordedVideoFilterOutput, VideoFilterOutput};
pub use source::{
    NativeNormalizedPacket, NativePacketStream, NativePacketStreamPoll, NativeSourceResource,
};
pub use video::{NativeVideoDecoder, NativeVideoFrame, VideoDecoderOutput};

use std::collections::BTreeMap;
use std::ffi::c_void;
use std::mem::{MaybeUninit, offset_of, size_of};
use std::path::Path;
use std::ptr;
use std::sync::Arc;
use std::time::Duration;

use player_plugin::author::*;
use player_plugin_abi::checked::*;
use player_plugin_abi::*;

/// Maximum number of factories a host can register in one plugin configuration.
pub const MAX_REGISTERED_AUDIO_PROCESSORS: usize = 64;
pub const DEFAULT_AUDIO_CALL_BUDGET: Duration = Duration::from_millis(20);

#[derive(Default)]
pub struct PluginRegistry {
    audio: BTreeMap<String, Arc<dyn AudioProcessorFactory>>,
}

impl PluginRegistry {
    pub fn register_static_audio_processor(
        &mut self,
        id: &str,
        factory: Arc<dyn AudioProcessorFactory>,
    ) -> PluginResult<()> {
        if !valid_plugin_id(id) || self.audio.contains_key(id) {
            return Err(PluginError::Contract(
                "invalid or duplicate plugin identity",
            ));
        }
        if self.audio.len() >= MAX_REGISTERED_AUDIO_PROCESSORS {
            return Err(PluginError::Status(VESPER_ERR_EXHAUSTED));
        }
        self.audio.insert(id.to_owned(), factory);
        Ok(())
    }

    pub fn register_native_audio_processor(&mut self, plugin: &NativePlugin) -> PluginResult<()> {
        let factory = plugin.audio_processor(DEFAULT_AUDIO_CALL_BUDGET)?;
        self.register_static_audio_processor(plugin.id(), factory)
    }

    pub fn audio_processor(&self, id: &str) -> Option<&Arc<dyn AudioProcessorFactory>> {
        self.audio.get(id)
    }
}

struct Root(VesperPluginRoot);

// SAFETY: the root contract supports concurrent cold calls and serial use of
// each independent session. Identity strings and tables remain immutable.
unsafe impl Send for Root {}
// SAFETY: a root outlives every call and session using it via shared ownership.
unsafe impl Sync for Root {}

impl Drop for Root {
    fn drop(&mut self) {
        if let Some(destroy) = self.0.destroy_owner {
            // SAFETY: the successful entry transferred its unique root owner.
            unsafe { destroy(self.0.owner) };
        }
    }
}

#[derive(Clone)]
pub struct NativePlugin {
    root: Arc<Root>,
    id: String,
    name: String,
    interfaces: Vec<VesperInterfaceId>,
}

impl NativePlugin {
    /// Loads an explicitly selected local native module on desktop.
    ///
    /// # Safety
    /// The caller trusts the module, dependencies and ABI implementation to run
    /// in-process. Validation detects protocol violations in readable values.
    pub unsafe fn load(path: impl AsRef<Path>) -> Result<Self, LoadError> {
        // SAFETY: caller authorizes executing a trusted native module.
        let entry = unsafe { library::entry(path.as_ref()) }?;
        // SAFETY: library::entry pins the module and resolves the native symbol only.
        unsafe { Self::from_entry(entry) }.map_err(LoadError::Contract)
    }

    /// Adopts a root from resident native code. A host kit can pass an entry
    /// linked at build time through its native bridge.
    ///
    /// # Safety
    /// The entry and all callbacks must implement the native contract, including
    /// entry ownership, memory validity and concurrent root calls. Their code
    /// must stay resident for the process lifetime.
    pub unsafe fn from_entry(entry: VesperPluginEntryPoint) -> PluginResult<Self> {
        // SAFETY: caller guarantees the entry signature and root memory contract.
        let raw = unsafe { entry() };
        // SAFETY: caller guarantees at least a readable leading size at non-null raw.
        let root = unsafe { read_sized(raw) }?;
        if root.abi_major != VESPER_PLUGIN_ABI_MAJOR {
            return Err(PluginError::Contract("incompatible plugin ABI major"));
        }
        // Take ownership before reading metadata or calling the optional interfaces.
        let root = Arc::new(Root(root));
        let raw = &root.0;
        if raw.interface_count > VESPER_MAX_INTERFACES_PER_PLUGIN || raw.query_interface.is_none() {
            return Err(PluginError::Contract("invalid root interface enumeration"));
        }
        // SAFETY: root owns both immutable slices for its lifetime.
        let (id, name) = unsafe {
            (
                read_bytes(raw.plugin_id, 255)?,
                read_bytes(raw.plugin_name, 255)?,
            )
        };
        let id = std::str::from_utf8(id)
            .ok()
            .filter(|id| valid_plugin_id(id))
            .ok_or(PluginError::Contract("invalid plugin ID"))?
            .to_owned();
        let name = std::str::from_utf8(name)
            .ok()
            .filter(|name| !name.is_empty())
            .ok_or(PluginError::Contract("invalid plugin name"))?
            .to_owned();
        let interface_at = raw
            .interface_at
            .ok_or(PluginError::Contract("missing interface_at"))?;
        let mut interfaces = Vec::with_capacity(raw.interface_count as usize);
        for index in 0..raw.interface_count {
            let mut id = VesperInterfaceId::default();
            // SAFETY: the owner is live and id is a writable fixed-size output.
            check_status(unsafe { interface_at(raw.owner, index, &mut id) })?;
            if id.bytes == [0; 16] || interfaces.contains(&id) {
                return Err(PluginError::Contract("invalid or duplicate interface ID"));
            }
            interfaces.push(id);
        }
        Ok(Self {
            root,
            id,
            name,
            interfaces,
        })
    }

    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn interface_ids(&self) -> &[VesperInterfaceId] {
        &self.interfaces
    }

    pub fn abi_minor(&self) -> u16 {
        self.root.0.abi_minor
    }

    fn query(&self, id: &VesperInterfaceId) -> PluginResult<*const c_void> {
        if !self.interfaces.contains(id) {
            return Err(PluginError::Status(VESPER_ERR_UNSUPPORTED));
        }
        let root = &self.root.0;
        let query = root
            .query_interface
            .ok_or(PluginError::Contract("missing query_interface"))?;
        let mut out = ptr::null();
        // SAFETY: the root owner and queried ID are live and out is writable.
        check_status(unsafe { query(root.owner, id, &mut out) })?;
        Ok(out)
    }

    pub fn audio_processor(
        &self,
        call_budget: Duration,
    ) -> PluginResult<Arc<dyn AudioProcessorFactory>> {
        let table = self
            .query(&VESPER_AUDIO_PROCESSOR_ID)?
            .cast::<VesperAudioProcessorVtable>();
        // SAFETY: the queried UUID identifies the audio ABI table. All optional
        // fields after the required prefix are nullable C function pointers.
        let table = unsafe {
            read_vtable(
                table,
                offset_of!(VesperAudioProcessorVtable, get_last_error),
            )
        }?;
        let factory = audio::Factory::new(Arc::clone(&self.root), table, call_budget)?;
        Ok(Arc::new(factory))
    }

    /// Opens a decoder that does not require a host GPU device at creation.
    pub fn open_video_decoder(
        &self,
        codec_fourcc: u32,
        extradata: &[u8],
        params: &[u8],
        call_budget: Duration,
    ) -> PluginResult<NativeVideoDecoder> {
        NativeVideoDecoder::open(
            Arc::clone(&self.root),
            self.video_decoder_table()?,
            None,
            codec_fourcc,
            extradata,
            params,
            call_budget,
        )
    }

    /// Opens a decoder against a host-owned GPU device.
    ///
    /// # Safety
    ///
    /// The tagged GPU members must be valid for the selected backend. The device
    /// and any context used by the decoder must outlive its session.
    pub unsafe fn open_video_decoder_with_device(
        &self,
        gpu: &VesperGpuContext,
        codec_fourcc: u32,
        extradata: &[u8],
        params: &[u8],
        call_budget: Duration,
    ) -> PluginResult<NativeVideoDecoder> {
        NativeVideoDecoder::open(
            Arc::clone(&self.root),
            self.video_decoder_table()?,
            Some(gpu),
            codec_fourcc,
            extradata,
            params,
            call_budget,
        )
    }

    fn video_decoder_table(&self) -> PluginResult<VesperVideoDecoderVtable> {
        let table = self
            .query(&VESPER_VIDEO_DECODER_ID)?
            .cast::<VesperVideoDecoderVtable>();
        // SAFETY: the queried interface identifies this table and its nullable suffix.
        unsafe { read_vtable(table, offset_of!(VesperVideoDecoderVtable, get_last_error)) }
    }
}

unsafe fn read_vtable<T: Copy>(raw: *const T, required: usize) -> PluginResult<T> {
    // SAFETY: caller supplies the ABI table pointer and the required prefix size.
    unsafe { require_size(raw, required) }?;
    // SAFETY: the size check guarantees the leading u32 is readable.
    let size = unsafe { raw.cast::<u32>().read() } as usize;
    let bytes = size.min(size_of::<T>());
    // Native vtable suffixes consist of pointer-sized nullable callbacks.
    if !(bytes - required).is_multiple_of(size_of::<*const c_void>()) {
        return Err(PluginError::Contract(
            "native vtable ends inside a callback",
        ));
    }
    let mut table = MaybeUninit::<T>::zeroed();
    // SAFETY: the declared prefix contains whole fields. The omitted suffix is nullable.
    unsafe {
        ptr::copy_nonoverlapping(raw.cast::<u8>(), table.as_mut_ptr().cast::<u8>(), bytes);
        Ok(table.assume_init())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("runtime native module loading is unsupported on this host")]
    UnsupportedHost,
    #[error("failed to resolve the local native module: {0}")]
    Path(#[from] std::io::Error),
    #[error("failed to open the native module: {0}")]
    Library(#[from] libloading::Error),
    #[error("failed to resolve vesper_native_plugin_entry: {0}")]
    Entry(#[source] libloading::Error),
    #[error(transparent)]
    Contract(#[from] PluginError),
}

#[cfg(test)]
mod vtable_tests {
    use super::*;

    unsafe extern "C" fn wait_for_update(_: *mut VesperSession, _: u32) -> VesperStatus {
        VESPER_TRY_AGAIN
    }

    #[test]
    fn reads_available_optional_callbacks_from_a_shorter_vtable() {
        let required = offset_of!(VesperSourceNormalizerResourceVtable, get_last_error);
        let mut source = VesperSourceNormalizerResourceVtable {
            struct_size: offset_of!(VesperSourceNormalizerResourceVtable, cancel) as u32,
            query_capabilities_json: None,
            open: None,
            get_resource_json: None,
            destroy: None,
            get_last_error: None,
            wait_for_update: Some(wait_for_update),
            cancel: None,
        };
        // SAFETY: the synthetic table is aligned and backed by a full allocation.
        let parsed = unsafe { read_vtable(&source, required) }.expect("valid vtable prefix");
        assert!(parsed.wait_for_update.is_some());
        assert!(parsed.cancel.is_none());

        source.struct_size -= 1;
        // SAFETY: the declared size claims a partial callback but remains readable.
        let parsed = unsafe { read_vtable(&source, required) };
        assert!(matches!(
            parsed,
            Err(PluginError::Contract(
                "native vtable ends inside a callback"
            ))
        ));
    }
}
