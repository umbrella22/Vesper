use std::mem::ManuallyDrop;
use std::path::Path;
use std::sync::Arc;

use libloading::Library;
use player_plugin_abi::{VESPER_PLUGIN_ENTRY_SYMBOL, VesperPluginEntryPoint};
use thiserror::Error;

use crate::LoadedNativePlugin;

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
    #[error("native ABI plugin violates the contract: {0}")]
    NativeContract(#[from] player_plugin::author::PluginError),
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
        let library = LibraryHolder::open(path)?;
        // SAFETY: the admitted module remains resident and the symbol has the native signature.
        let entry = unsafe {
            library
                .library
                .get::<VesperPluginEntryPoint>(VESPER_PLUGIN_ENTRY_SYMBOL.to_bytes_with_nul())
        }
        .map_err(|source| PluginLoadError::ResolveEntrySymbol {
            symbol: "vesper_native_plugin_entry",
            source,
        })?;
        // SAFETY: the entry and its callbacks stay mapped for the life of the process.
        let plugin = unsafe { crate::native::NativePlugin::from_entry(*entry) }?;
        Ok(Self::from_native_plugin(plugin)?)
    }
}

#[derive(Debug)]
pub(crate) struct LibraryHolder {
    // Plugins may register thread-local destructors through native
    // dependencies. Keep the library mapped for the process lifetime.
    library: ManuallyDrop<Library>,
}

impl LibraryHolder {
    fn open(path: &Path) -> Result<Arc<Self>, PluginLoadError> {
        let path_string = path.display().to_string();
        // SAFETY: the caller selects the native library. It is immediately
        // placed in `LibraryHolder`, whose process-lifetime policy keeps all
        // code and callback pointers mapped.
        let library =
            unsafe { Library::new(path) }.map_err(|source| PluginLoadError::OpenLibrary {
                path: path_string,
                source,
            })?;
        Ok(Arc::new(Self {
            library: ManuallyDrop::new(library),
        }))
    }
}

#[cfg(all(test, unix, not(target_os = "android")))]
mod tests {
    use std::process::Command;

    use super::*;

    #[test]
    fn native_admission_rejects_retired_entry_symbols() {
        let fixture = tempfile::tempdir().expect("native fixture directory");
        let library_flag = if cfg!(target_vendor = "apple") {
            "-dynamiclib"
        } else {
            "-shared"
        };
        for (index, retired_entry) in ["vesper_plugin_entry", "vesper_plugin_entry_v2"]
            .into_iter()
            .enumerate()
        {
            let source = fixture.path().join(format!("retired-{index}.c"));
            let library = fixture.path().join(format!("retired-{index}.shared"));
            std::fs::write(
                &source,
                format!("#include <stdlib.h>\nvoid *{retired_entry}(void) {{ abort(); }}\n"),
            )
            .expect("write retired entry fixture");
            assert!(
                Command::new("cc")
                    .args([library_flag, "-fPIC", "-o"])
                    .arg(&library)
                    .arg(&source)
                    .status()
                    .expect("compile retired entry fixture")
                    .success()
            );

            for result in [
                LoadedNativePlugin::load_development(&library),
                LoadedNativePlugin::load_host_verified(&library),
            ] {
                assert!(matches!(
                    result,
                    Err(PluginLoadError::ResolveEntrySymbol {
                        symbol: "vesper_native_plugin_entry",
                        ..
                    })
                ));
            }
            // SAFETY: the fixture contains only the retired export. It has no constructors.
            let result = unsafe { crate::native::NativePlugin::load(&library) };
            assert!(matches!(result, Err(crate::native::LoadError::Entry(_))));
        }
    }
}
