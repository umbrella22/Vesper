use super::*;

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
pub(super) unsafe fn entry(path: &Path) -> Result<VesperPluginEntryPoint, LoadError> {
    use std::path::PathBuf;
    use std::sync::Mutex;

    const MAX_RESIDENT_MODULES: usize = 64;
    static MODULES: Mutex<Vec<(PathBuf, Option<Arc<libloading::Library>>)>> =
        Mutex::new(Vec::new());
    let path = path.canonicalize()?;
    let existing = {
        let mut modules = MODULES.lock().unwrap_or_else(|e| e.into_inner());
        match modules.iter().find(|(loaded, _)| loaded == &path) {
            Some((_, Some(module))) => Some(Arc::clone(module)),
            Some((_, None)) => return Err(PluginError::Status(VESPER_TRY_AGAIN).into()),
            None => {
                if modules.len() >= MAX_RESIDENT_MODULES {
                    return Err(PluginError::Status(VESPER_ERR_EXHAUSTED).into());
                }
                modules.push((path.clone(), None));
                None
            }
        }
    };
    let module = if let Some(existing) = existing {
        existing
    } else {
        // SAFETY: the caller authorizes execution of this trusted local module.
        let opened = unsafe { libloading::Library::new(&path) }.map(Arc::new);
        let mut modules = MODULES.lock().unwrap_or_else(|e| e.into_inner());
        // This call exclusively owns the pending entry for the canonical path.
        if let Some(index) = modules.iter().position(|(candidate, _)| candidate == &path) {
            match &opened {
                Ok(module) => modules[index].1 = Some(Arc::clone(module)),
                Err(_) => {
                    modules.swap_remove(index);
                }
            }
        }
        drop(modules);
        opened?
    };
    // SAFETY: the native symbol has the ABI signature accepted by the caller.
    // MODULES retains the module for the lifetime of the process, including
    // callbacks held by GPU work after the root and session have been destroyed.
    let entry = unsafe {
        module.get::<VesperPluginEntryPoint>(VESPER_PLUGIN_ENTRY_SYMBOL.to_bytes_with_nul())
    }
    .map_err(LoadError::Entry)?;
    Ok(*entry)
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
pub(super) unsafe fn entry(_path: &Path) -> Result<VesperPluginEntryPoint, LoadError> {
    Err(LoadError::UnsupportedHost)
}
