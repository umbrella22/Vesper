use std::mem::ManuallyDrop;
use std::path::Path;
use std::sync::Arc;

use libloading::Library;
use thiserror::Error;

use crate::native_abi::CheckedPluginRoot;
use crate::{LoadedNativePlugin, NativePluginContractError};

#[derive(Debug, Error)]
pub enum PluginLoadError {
    #[error("failed to open plugin library at {path}: {source}")]
    OpenLibrary {
        path: String,
        #[source]
        source: libloading::Error,
    },
    #[error("failed to resolve plugin entry symbol `{symbol}`: {source}")]
    ResolveEntrySymbol {
        symbol: &'static str,
        #[source]
        source: libloading::Error,
    },
    #[error("native plugin root violates the ABI contract: {0}")]
    NativeContract(#[from] NativePluginContractError),
}

impl LoadedNativePlugin {
    /// Loads and validates one unsigned native plugin for explicit
    /// development or inspection workflows.
    ///
    /// The mapped library remains alive for the process lifetime, including
    /// when symbol resolution or root validation fails after `dlopen`.
    pub fn load_development(path: impl AsRef<Path>) -> Result<Self, PluginLoadError> {
        let path = path.as_ref();
        tracing::warn!(
            path = %path.display(),
            "loading an unsigned raw Native plugin library under explicit development policy"
        );
        Self::load_unchecked(path)
    }

    pub(crate) fn load_host_verified(path: impl AsRef<Path>) -> Result<Self, PluginLoadError> {
        Self::load_unchecked(path.as_ref())
    }

    fn load_unchecked(path: &Path) -> Result<Self, PluginLoadError> {
        let path_string = path.display().to_string();
        // SAFETY: the caller selects the native library. It is immediately
        // placed in `LibraryHolder`, whose process-lifetime policy keeps all
        // code and callback pointers mapped.
        let library =
            unsafe { Library::new(path) }.map_err(|source| PluginLoadError::OpenLibrary {
                path: path_string,
                source,
            })?;
        let library = Arc::new(LibraryHolder {
            library: ManuallyDrop::new(library),
        });

        // SAFETY: the symbol name and signature come from the raw native ABI
        // crate. There is deliberately no legacy-symbol fallback.
        let entry = unsafe {
            library
                .library
                .get::<player_plugin_abi::VesperPluginEntryPoint>(
                    player_plugin_abi::VESPER_PLUGIN_ENTRY_SYMBOL,
                )
        }
        .map_err(|source| PluginLoadError::ResolveEntrySymbol {
            symbol: player_plugin_abi::VESPER_PLUGIN_ENTRY_SYMBOL_NAME,
            source,
        })?;

        // SAFETY: the resolved plugin entry transfers one root owner to the host.
        let root = unsafe { entry() };
        let checked =
            // SAFETY: the entry contract permits null or a readable native root;
            // validation owns all subsequent pointer and size checks.
            unsafe { CheckedPluginRoot::from_raw(root, Some(library)) }?;
        Ok(Self::from_checked(checked))
    }
}

#[derive(Debug)]
pub(crate) struct LibraryHolder {
    // Plugins may register thread-local destructors through native
    // dependencies. Keep the library mapped for the process lifetime.
    #[allow(dead_code)]
    pub(crate) library: ManuallyDrop<Library>,
}
