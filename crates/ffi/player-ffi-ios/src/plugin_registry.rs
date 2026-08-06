use std::collections::{HashMap, HashSet};
use std::ffi::c_char;
use std::fs;
use std::path::{Path, PathBuf};
use std::ptr;
use std::slice;
use std::sync::{Arc, Mutex, OnceLock};

use player_ffi_common::{clear_c_string_output, write_c_string_output};
use player_plugin::{PluginReference, PluginTransport};
use player_plugin_loader::{
    EmbeddedAppleCodeSignatureValidation, EmbeddedPluginArtifact, EmbeddedPluginIntegrity,
    EmbeddedPluginLocator, EmbeddedPluginRegistry, MAX_EMBEDDED_PLUGIN_ARTIFACTS,
    MAX_EMBEDDED_PLUGIN_REGISTRY_FRAGMENTS, PluginRegistry,
};
use serde::{Deserialize, Serialize};

use crate::{
    HandleRegistry, PlayerFfiCallStatus, PlayerFfiError, PlayerFfiErrorCode, ffi_call, ffi_void,
    free_c_string, lock_registry, owned_api_error, write_error,
};

const MAX_FRAGMENT_SET_JSON_BYTES: usize = 8 * 1024 * 1024;
const MAX_PLUGIN_REFERENCE_SET_BYTES: usize = 1024 * 1024;
const MAX_RESOLVED_FRAMEWORK_SET_BYTES: usize = 1024 * 1024;
const MAX_ACTIVE_PLUGIN_PLANS: usize = 256;
const MAX_ACTIVE_PLUGIN_REGISTRIES: usize = 256;

#[derive(Debug)]
struct IosPluginPlan {
    embedded: EmbeddedPluginRegistry,
    references: Vec<PluginReference>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct IosPluginResolution<'a> {
    plugin_id: &'a str,
    framework_name: &'a str,
    bundle_identifier: &'a str,
    validation: &'static str,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ResolvedFrameworkSet {
    frameworks_root: String,
    frameworks: Vec<ResolvedFramework>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ResolvedFramework {
    plugin_id: String,
    framework_name: String,
    bundle_identifier: String,
    framework_path: String,
    binary_path: String,
}

static PLUGIN_PLANS: OnceLock<Mutex<HandleRegistry<Arc<IosPluginPlan>>>> = OnceLock::new();
static PLUGIN_REGISTRIES: OnceLock<Mutex<HandleRegistry<Arc<PluginRegistry>>>> = OnceLock::new();

fn plugin_plans() -> &'static Mutex<HandleRegistry<Arc<IosPluginPlan>>> {
    PLUGIN_PLANS.get_or_init(|| Mutex::new(HandleRegistry::default()))
}

fn plugin_registries() -> &'static Mutex<HandleRegistry<Arc<PluginRegistry>>> {
    PLUGIN_REGISTRIES.get_or_init(|| Mutex::new(HandleRegistry::default()))
}

fn current_ios_registry_target() -> Result<(&'static str, &'static str), String> {
    if cfg!(all(
        target_os = "ios",
        target_arch = "aarch64",
        target_abi = "sim"
    )) {
        Ok(("aarch64-apple-ios-sim", "arm64"))
    } else if cfg!(all(
        target_os = "ios",
        target_arch = "aarch64",
        not(target_abi = "sim")
    )) {
        Ok(("aarch64-apple-ios", "arm64"))
    } else {
        Err("iOS embedded plugins require an arm64 device or Apple Silicon Simulator".to_owned())
    }
}

fn parse_plugin_plan(
    fragment_set_json: &[u8],
    references_json: &[u8],
    expected_target: &str,
    expected_architecture: &str,
) -> Result<IosPluginPlan, String> {
    if fragment_set_json.len() > MAX_FRAGMENT_SET_JSON_BYTES {
        return Err(format!(
            "iOS plugin fragment set is {} bytes; maximum is {} bytes",
            fragment_set_json.len(),
            MAX_FRAGMENT_SET_JSON_BYTES
        ));
    }
    if references_json.len() > MAX_PLUGIN_REFERENCE_SET_BYTES {
        return Err(format!(
            "iOS plugin reference set is {} bytes; maximum is {} bytes",
            references_json.len(),
            MAX_PLUGIN_REFERENCE_SET_BYTES
        ));
    }
    let fragments = serde_json::from_slice::<Vec<String>>(fragment_set_json)
        .map_err(|error| format!("invalid iOS plugin fragment-set JSON: {error}"))?;
    if fragments.len() > MAX_EMBEDDED_PLUGIN_REGISTRY_FRAGMENTS {
        return Err(format!(
            "iOS plugin fragment set contains {} entries; maximum is {}",
            fragments.len(),
            MAX_EMBEDDED_PLUGIN_REGISTRY_FRAGMENTS
        ));
    }
    let references = serde_json::from_slice::<Vec<PluginReference>>(references_json)
        .map_err(|error| format!("invalid iOS plugin reference JSON: {error}"))?;
    if references.len() > MAX_EMBEDDED_PLUGIN_ARTIFACTS {
        return Err(format!(
            "iOS plugin reference set contains {} entries; maximum is {}",
            references.len(),
            MAX_EMBEDDED_PLUGIN_ARTIFACTS
        ));
    }
    let embedded = EmbeddedPluginRegistry::parse_fragments(
        fragments.iter().map(String::as_bytes),
        expected_target,
        expected_architecture,
    )
    .map_err(|error| error.to_string())?;
    embedded
        .select_native_artifacts(&references)
        .map_err(|error| error.to_string())?;
    Ok(IosPluginPlan {
        embedded,
        references,
    })
}

fn plan_resolutions_json(plan: &IosPluginPlan) -> Result<String, String> {
    let artifacts = plan
        .embedded
        .select_native_artifacts(&plan.references)
        .map_err(|error| error.to_string())?;
    let resolutions = artifacts
        .into_iter()
        .map(|artifact| {
            let EmbeddedPluginLocator::AppleFramework {
                name,
                bundle_identifier,
            } = artifact.locator()
            else {
                return Err(format!(
                    "iOS plugin `{}` does not use an Apple framework locator",
                    artifact.plugin_id()
                ));
            };
            if artifact.integrity().apple_code_signature_validation()
                != Some(EmbeddedAppleCodeSignatureValidation::SameTeamAsHostOrSimulatorAdHoc)
            {
                return Err(format!(
                    "iOS plugin `{}` does not declare the required Apple signature policy",
                    artifact.plugin_id()
                ));
            }
            Ok(IosPluginResolution {
                plugin_id: artifact.plugin_id(),
                framework_name: name,
                bundle_identifier,
                validation: "same-team-as-host-or-simulator-ad-hoc",
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    serde_json::to_string(&resolutions)
        .map_err(|error| format!("failed to encode iOS plugin resolutions: {error}"))
}

fn validate_resolved_frameworks(
    plan: &IosPluginPlan,
    json: &[u8],
) -> Result<HashMap<String, PathBuf>, String> {
    if json.len() > MAX_RESOLVED_FRAMEWORK_SET_BYTES {
        return Err(format!(
            "resolved iOS framework set is {} bytes; maximum is {} bytes",
            json.len(),
            MAX_RESOLVED_FRAMEWORK_SET_BYTES
        ));
    }
    let resolved = serde_json::from_slice::<ResolvedFrameworkSet>(json)
        .map_err(|error| format!("invalid resolved iOS framework JSON: {error}"))?;
    let selected = plan
        .embedded
        .select_native_artifacts(&plan.references)
        .map_err(|error| error.to_string())?;
    if resolved.frameworks.len() != selected.len() {
        return Err(format!(
            "resolved iOS framework count {} does not match selected plugin count {}",
            resolved.frameworks.len(),
            selected.len()
        ));
    }
    if selected.is_empty() {
        return Ok(HashMap::new());
    }

    let root = canonical_directory_without_symlink(
        Path::new(&resolved.frameworks_root),
        "iOS private frameworks directory",
    )?;
    let mut by_plugin = HashMap::with_capacity(resolved.frameworks.len());
    let mut framework_paths = HashSet::with_capacity(resolved.frameworks.len());
    for record in resolved.frameworks {
        if by_plugin.contains_key(&record.plugin_id) {
            return Err(format!(
                "duplicate resolved iOS framework for plugin `{}`",
                record.plugin_id
            ));
        }
        let artifact = selected
            .iter()
            .find(|artifact| artifact.plugin_id() == record.plugin_id)
            .ok_or_else(|| {
                format!(
                    "resolved iOS framework contains unselected plugin `{}`",
                    record.plugin_id
                )
            })?;
        let EmbeddedPluginLocator::AppleFramework {
            name,
            bundle_identifier,
        } = artifact.locator()
        else {
            return Err(format!(
                "iOS plugin `{}` does not use an Apple framework locator",
                artifact.plugin_id()
            ));
        };
        if record.framework_name != *name || record.bundle_identifier != *bundle_identifier {
            return Err(format!(
                "resolved iOS framework metadata does not match plugin `{}`",
                artifact.plugin_id()
            ));
        }
        let expected_framework = root.join(format!("{name}.framework"));
        let framework = canonical_directory_without_symlink(
            Path::new(&record.framework_path),
            "resolved iOS plugin framework",
        )?;
        let expected_framework = fs::canonicalize(&expected_framework).map_err(|error| {
            format!(
                "failed to resolve expected iOS plugin framework '{}': {error}",
                expected_framework.display()
            )
        })?;
        if framework != expected_framework || framework.parent() != Some(root.as_path()) {
            return Err(format!(
                "resolved iOS plugin framework is not the direct packaged `{name}.framework` child"
            ));
        }
        if !framework_paths.insert(framework.clone()) {
            return Err(format!(
                "multiple iOS plugin records resolve to framework '{}'",
                framework.display()
            ));
        }

        let expected_binary = framework.join(name);
        let binary = canonical_regular_file_without_symlink(
            Path::new(&record.binary_path),
            "resolved iOS plugin binary",
        )?;
        let expected_binary = fs::canonicalize(&expected_binary).map_err(|error| {
            format!(
                "failed to resolve expected iOS plugin binary '{}': {error}",
                expected_binary.display()
            )
        })?;
        if binary != expected_binary || binary.parent() != Some(framework.as_path()) {
            return Err(format!(
                "resolved iOS plugin binary is not `{name}.framework/{name}`"
            ));
        }
        by_plugin.insert(record.plugin_id, binary);
    }
    Ok(by_plugin)
}

fn canonical_directory_without_symlink(path: &Path, label: &str) -> Result<PathBuf, String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("failed to inspect {label} '{}': {error}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(format!(
            "{label} '{}' is not a real directory",
            path.display()
        ));
    }
    fs::canonicalize(path)
        .map_err(|error| format!("failed to resolve {label} '{}': {error}", path.display()))
}

fn canonical_regular_file_without_symlink(path: &Path, label: &str) -> Result<PathBuf, String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("failed to inspect {label} '{}': {error}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(format!(
            "{label} '{}' is not a regular file",
            path.display()
        ));
    }
    fs::canonicalize(path)
        .map_err(|error| format!("failed to resolve {label} '{}': {error}", path.display()))
}

fn load_plugin_registry(
    plan: &IosPluginPlan,
    resolved_frameworks_json: &[u8],
) -> Result<Arc<PluginRegistry>, String> {
    if !cfg!(target_os = "ios") {
        return Err("iOS embedded plugin loading is unavailable on this host".to_owned());
    }
    let paths = validate_resolved_frameworks(plan, resolved_frameworks_json)?;
    let registry = plan
        .embedded
        .load_native_selected_with_platform_integrity(
            &plan.references,
            |locator| match locator {
                EmbeddedPluginLocator::AppleFramework { .. } => plan
                    .embedded
                    .artifacts()
                    .iter()
                    .find(|artifact| artifact.locator() == locator)
                    .and_then(|artifact| paths.get(artifact.plugin_id()))
                    .cloned()
                    .ok_or_else(|| "resolved iOS plugin path is missing".to_owned()),
                EmbeddedPluginLocator::AndroidNativeLibrary { .. } => {
                    Err("Android locator is invalid for iOS".to_owned())
                }
            },
            |path, artifact| verify_ios_platform_loading_boundary(path, artifact, &paths),
        )
        .map_err(|error| error.to_string())?;
    Ok(Arc::new(registry))
}

fn verify_ios_platform_loading_boundary(
    path: &Path,
    artifact: &EmbeddedPluginArtifact,
    paths: &HashMap<String, PathBuf>,
) -> Result<(), String> {
    if artifact.transport() != PluginTransport::Native {
        return Err("iOS embedded plugins require Native transport".to_owned());
    }
    if !matches!(
        artifact.integrity(),
        EmbeddedPluginIntegrity::AppleCodeSignature {
            validation: EmbeddedAppleCodeSignatureValidation::SameTeamAsHostOrSimulatorAdHoc
        }
    ) {
        return Err("iOS embedded plugin has an unsupported integrity policy".to_owned());
    }
    if paths.get(artifact.plugin_id()).map(PathBuf::as_path) != Some(path) {
        return Err("iOS embedded plugin path changed after resolution".to_owned());
    }

    // Public iOS SDKs do not expose macOS Code Signing Services. The final-app
    // release verifier records the same-team (or Simulator ad-hoc) evidence,
    // this function binds loading to that sealed Frameworks child, and the
    // immediately following dlopen is code-signature enforced by iOS dyld.
    Ok(())
}

fn register_plan(plan: Arc<IosPluginPlan>) -> Result<u64, String> {
    let mut plans = lock_registry(plugin_plans())
        .map_err(|_| "iOS plugin plan registry lock failed".to_owned())?;
    if plans.len() >= MAX_ACTIVE_PLUGIN_PLANS {
        return Err(format!(
            "active iOS plugin plan count reached {MAX_ACTIVE_PLUGIN_PLANS}"
        ));
    }
    Ok(plans.insert(plan))
}

fn clone_plan(handle: u64) -> Result<Arc<IosPluginPlan>, String> {
    let plans = lock_registry(plugin_plans())
        .map_err(|_| "iOS plugin plan registry lock failed".to_owned())?;
    plans
        .get(handle)
        .cloned()
        .ok_or_else(|| "invalid iOS plugin plan handle".to_owned())
}

fn dispose_plan(handle: u64) {
    let plan = lock_registry(plugin_plans())
        .ok()
        .and_then(|mut plans| plans.remove(handle));
    drop(plan);
}

fn register_plugin_registry(registry: Arc<PluginRegistry>) -> Result<u64, String> {
    let mut registries = lock_registry(plugin_registries())
        .map_err(|_| "iOS plugin registry lock failed".to_owned())?;
    if registries.len() >= MAX_ACTIVE_PLUGIN_REGISTRIES {
        return Err(format!(
            "active iOS plugin registry count reached {MAX_ACTIVE_PLUGIN_REGISTRIES}"
        ));
    }
    Ok(registries.insert(registry))
}

#[cfg(test)]
pub(crate) fn register_test_plugin_registry(registry: PluginRegistry) -> Result<u64, String> {
    register_plugin_registry(Arc::new(registry))
}

pub(crate) fn clone_plugin_registry(handle: u64) -> Result<Arc<PluginRegistry>, String> {
    let registries = lock_registry(plugin_registries())
        .map_err(|_| "iOS plugin registry lock failed".to_owned())?;
    registries
        .get(handle)
        .cloned()
        .ok_or_else(|| "invalid iOS plugin registry handle".to_owned())
}

fn dispose_plugin_registry(handle: u64) {
    let registry = lock_registry(plugin_registries())
        .ok()
        .and_then(|mut registries| registries.remove(handle));
    drop(registry);
}

unsafe fn read_bounded_bytes<'a>(
    value: *const u8,
    len: usize,
    maximum: usize,
    field: &str,
) -> Result<&'a [u8], PlayerFfiError> {
    if len > maximum {
        return Err(owned_api_error(
            PlayerFfiErrorCode::InvalidArgument,
            &format!("{field} is {len} bytes; maximum is {maximum} bytes"),
        ));
    }
    if len == 0 {
        return Ok(&[]);
    }
    if value.is_null() {
        return Err(owned_api_error(
            PlayerFfiErrorCode::NullPointer,
            &format!("{field} was null for a non-empty payload"),
        ));
    }
    // SAFETY: the caller guarantees `value` points to `len` readable bytes;
    // the size is bounded above before constructing the slice.
    Ok(unsafe { slice::from_raw_parts(value, len) })
}

/// Creates a non-executing iOS plugin plan from packaged registry fragments
/// and explicit references.
///
/// # Safety
///
/// Non-null byte pointers must remain readable for their corresponding lengths
/// for the duration of the call. Output pointers must be writable.
pub(crate) unsafe fn player_ffi_ios_plugin_plan_create_impl(
    fragment_set_json: *const u8,
    fragment_set_json_len: usize,
    references_json: *const u8,
    references_json_len: usize,
    out_handle: *mut u64,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        if out_handle.is_null() {
            write_error(
                out_error,
                owned_api_error(PlayerFfiErrorCode::NullPointer, "out_handle was null"),
            );
            return PlayerFfiCallStatus::Error;
        }
        // SAFETY: `out_handle` is non-null and writable by contract.
        unsafe { ptr::write(out_handle, 0) };
        // SAFETY: caller upholds the byte-buffer contracts documented above.
        let fragments = match unsafe {
            read_bounded_bytes(
                fragment_set_json,
                fragment_set_json_len,
                MAX_FRAGMENT_SET_JSON_BYTES,
                "fragment_set_json",
            )
        } {
            Ok(value) => value,
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };
        // SAFETY: caller upholds the byte-buffer contracts documented above.
        let references = match unsafe {
            read_bounded_bytes(
                references_json,
                references_json_len,
                MAX_PLUGIN_REFERENCE_SET_BYTES,
                "references_json",
            )
        } {
            Ok(value) => value,
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };
        let (target, architecture) = match current_ios_registry_target() {
            Ok(target) => target,
            Err(message) => {
                write_error(
                    out_error,
                    owned_api_error(PlayerFfiErrorCode::Unsupported, &message),
                );
                return PlayerFfiCallStatus::Error;
            }
        };
        let plan = match parse_plugin_plan(fragments, references, target, architecture) {
            Ok(plan) => Arc::new(plan),
            Err(message) => {
                write_error(
                    out_error,
                    owned_api_error(PlayerFfiErrorCode::InvalidArgument, &message),
                );
                return PlayerFfiCallStatus::Error;
            }
        };
        let handle = match register_plan(plan) {
            Ok(handle) => handle,
            Err(message) => {
                write_error(
                    out_error,
                    owned_api_error(PlayerFfiErrorCode::InvalidState, &message),
                );
                return PlayerFfiCallStatus::Error;
            }
        };
        // SAFETY: `out_handle` is non-null and writable by contract.
        unsafe { ptr::write(out_handle, handle) };
        PlayerFfiCallStatus::Ok
    })
}

/// Returns the selected Apple framework locators without loading code.
///
/// # Safety
///
/// `out_json` and `out_error` must be writable when non-null. The returned
/// string must be freed with `player_ffi_ios_plugin_string_free`.
pub(crate) unsafe fn player_ffi_ios_plugin_plan_resolutions_json_impl(
    handle: u64,
    out_json: *mut *mut c_char,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        if out_json.is_null() {
            write_error(
                out_error,
                owned_api_error(PlayerFfiErrorCode::NullPointer, "out_json was null"),
            );
            return PlayerFfiCallStatus::Error;
        }
        // SAFETY: `out_json` is non-null and writable by contract.
        unsafe { clear_c_string_output(out_json) };
        let plan = match clone_plan(handle) {
            Ok(plan) => plan,
            Err(message) => {
                write_error(
                    out_error,
                    owned_api_error(PlayerFfiErrorCode::InvalidArgument, &message),
                );
                return PlayerFfiCallStatus::Error;
            }
        };
        let json = match plan_resolutions_json(&plan) {
            Ok(json) => json,
            Err(message) => {
                write_error(
                    out_error,
                    owned_api_error(PlayerFfiErrorCode::InvalidState, &message),
                );
                return PlayerFfiCallStatus::Error;
            }
        };
        // SAFETY: `out_json` is non-null and writable by contract.
        unsafe { write_c_string_output(out_json, json) };
        PlayerFfiCallStatus::Ok
    })
}

/// Loads an iOS plugin registry from framework paths resolved for one plan.
///
/// # Safety
///
/// `resolved_frameworks_json` must remain readable for its length. Output
/// pointers must be writable when non-null.
pub(crate) unsafe fn player_ffi_ios_plugin_registry_load_impl(
    plan_handle: u64,
    resolved_frameworks_json: *const u8,
    resolved_frameworks_json_len: usize,
    out_handle: *mut u64,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        if out_handle.is_null() {
            write_error(
                out_error,
                owned_api_error(PlayerFfiErrorCode::NullPointer, "out_handle was null"),
            );
            return PlayerFfiCallStatus::Error;
        }
        // SAFETY: `out_handle` is non-null and writable by contract.
        unsafe { ptr::write(out_handle, 0) };
        // SAFETY: caller upholds the byte-buffer contract documented above.
        let resolved = match unsafe {
            read_bounded_bytes(
                resolved_frameworks_json,
                resolved_frameworks_json_len,
                MAX_RESOLVED_FRAMEWORK_SET_BYTES,
                "resolved_frameworks_json",
            )
        } {
            Ok(value) => value,
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };
        let plan = match clone_plan(plan_handle) {
            Ok(plan) => plan,
            Err(message) => {
                write_error(
                    out_error,
                    owned_api_error(PlayerFfiErrorCode::InvalidArgument, &message),
                );
                return PlayerFfiCallStatus::Error;
            }
        };
        let registry = match load_plugin_registry(&plan, resolved) {
            Ok(registry) => registry,
            Err(message) => {
                write_error(
                    out_error,
                    owned_api_error(PlayerFfiErrorCode::InvalidArgument, &message),
                );
                return PlayerFfiCallStatus::Error;
            }
        };
        let handle = match register_plugin_registry(registry) {
            Ok(handle) => handle,
            Err(message) => {
                write_error(
                    out_error,
                    owned_api_error(PlayerFfiErrorCode::InvalidState, &message),
                );
                return PlayerFfiCallStatus::Error;
            }
        };
        // SAFETY: `out_handle` is non-null and writable by contract.
        unsafe { ptr::write(out_handle, handle) };
        PlayerFfiCallStatus::Ok
    })
}

pub(crate) unsafe fn player_ffi_ios_plugin_plan_dispose_impl(handle: u64) {
    ffi_void(|| dispose_plan(handle));
}

pub(crate) unsafe fn player_ffi_ios_plugin_registry_dispose_impl(handle: u64) {
    ffi_void(|| dispose_plugin_registry(handle));
}

/// # Safety
///
/// `value` must be null or a Rust-owned string returned by an iOS plugin plan
/// API in this module.
pub(crate) unsafe fn player_ffi_ios_plugin_string_free_impl(value: *mut c_char) {
    ffi_void(|| {
        let mut value = value;
        free_c_string(&mut value);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apple_fragment() -> String {
        r#"{
            "schema_version":1,
            "target":"aarch64-apple-ios",
            "architecture":"arm64",
            "minimum_os":"17.0",
            "artifacts":[{
                "plugin_id":"dev.vesper.fixture",
                "transport":"native",
                "locator":{
                    "kind":"apple-framework",
                    "name":"VesperPluginFixture",
                    "bundle_identifier":"dev.vesper.plugin-fixture"
                },
                "integrity":{
                    "kind":"apple-code-signature",
                    "validation":"same-team-as-host-or-simulator-ad-hoc"
                },
                "package":{
                    "version":"1.2.3",
                    "publisher":"dev.vesper.publisher",
                    "descriptor_sha256":"0000000000000000000000000000000000000000000000000000000000000000"
                },
                "capabilities":[{
                    "interface_id":"e9479dbc-42d2-575e-b39e-a24bc512fbc7",
                    "instance_id":"dev.vesper.fixture.post-download",
                    "interface_major":1,
                    "interface_minor":0
                }]
            }]
        }"#
        .to_owned()
    }

    fn fixture_plan() -> IosPluginPlan {
        let fragments = serde_json::to_vec(&vec![apple_fragment()]).expect("fragment set");
        let references = br#"[{"pluginId":"dev.vesper.fixture","transport":"native"}]"#;
        parse_plugin_plan(&fragments, references, "aarch64-apple-ios", "arm64")
            .expect("plugin plan")
    }

    #[test]
    fn plan_returns_only_rust_validated_selected_locators() {
        let plan = fixture_plan();
        let resolutions: serde_json::Value =
            serde_json::from_str(&plan_resolutions_json(&plan).expect("resolution JSON"))
                .expect("resolution value");

        assert_eq!(resolutions.as_array().map(Vec::len), Some(1));
        assert_eq!(resolutions[0]["pluginId"], "dev.vesper.fixture");
        assert_eq!(resolutions[0]["frameworkName"], "VesperPluginFixture");
        assert_eq!(
            resolutions[0]["bundleIdentifier"],
            "dev.vesper.plugin-fixture"
        );
    }

    #[test]
    fn empty_plan_does_not_require_a_frameworks_directory() {
        let plan = parse_plugin_plan(b"[]", b"[]", "aarch64-apple-ios", "arm64")
            .expect("empty plugin plan");
        let resolved = br#"{"frameworksRoot":"","frameworks":[]}"#;

        assert!(
            validate_resolved_frameworks(&plan, resolved)
                .expect("empty resolved set")
                .is_empty()
        );
    }

    #[test]
    fn resolved_frameworks_must_be_direct_non_symlink_children() {
        let plan = fixture_plan();
        let directory = tempfile::tempdir().expect("temporary app");
        let root = directory.path().join("Frameworks");
        let framework = root.join("VesperPluginFixture.framework");
        fs::create_dir_all(&framework).expect("framework directory");
        let binary = framework.join("VesperPluginFixture");
        fs::write(&binary, b"fixture").expect("framework binary");
        let json = serde_json::json!({
            "frameworksRoot": root,
            "frameworks": [{
                "pluginId": "dev.vesper.fixture",
                "frameworkName": "VesperPluginFixture",
                "bundleIdentifier": "dev.vesper.plugin-fixture",
                "frameworkPath": framework,
                "binaryPath": binary,
            }],
        });

        let paths =
            validate_resolved_frameworks(&plan, &serde_json::to_vec(&json).expect("resolved JSON"))
                .expect("valid framework paths");
        assert_eq!(paths.len(), 1);

        let outside = directory.path().join("Outside.framework");
        fs::create_dir(&outside).expect("outside framework");
        let outside_binary = outside.join("VesperPluginFixture");
        fs::write(&outside_binary, b"fixture").expect("outside binary");
        let invalid = serde_json::json!({
            "frameworksRoot": root,
            "frameworks": [{
                "pluginId": "dev.vesper.fixture",
                "frameworkName": "VesperPluginFixture",
                "bundleIdentifier": "dev.vesper.plugin-fixture",
                "frameworkPath": outside,
                "binaryPath": outside_binary,
            }],
        });
        assert!(
            validate_resolved_frameworks(
                &plan,
                &serde_json::to_vec(&invalid).expect("invalid JSON")
            )
            .expect_err("outside framework must fail")
            .contains("direct packaged")
        );
    }

    #[test]
    fn plan_handles_are_generation_safe_and_retained_values_outlive_dispose() {
        let first = register_plan(Arc::new(fixture_plan())).expect("first plan");
        let retained = clone_plan(first).expect("retained plan");
        dispose_plan(first);
        assert!(clone_plan(first).is_err());
        assert_eq!(
            retained.embedded.artifacts()[0].plugin_id(),
            "dev.vesper.fixture"
        );

        let second = register_plan(Arc::new(fixture_plan())).expect("second plan");
        assert_ne!(first, second);
        dispose_plan(second);
    }
}
