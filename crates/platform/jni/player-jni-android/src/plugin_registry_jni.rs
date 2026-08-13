use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use jni::EnvUnowned;
use jni::errors::{Result as JniResult, ThrowRuntimeExAndDefault};
use jni::objects::{JClass, JObjectArray, JString};
use jni::sys::{jint, jlong};
use player_plugin::PluginReference;
use player_plugin_loader::{
    EmbeddedPluginLocator, EmbeddedPluginRegistry, MAX_ANDROID_PACKAGE_PATHS,
    MAX_EMBEDDED_PLUGIN_ARTIFACTS, PluginRegistry, resolve_android_native_library,
};

use crate::sessions::new_benchmark_sink_session_from_registry;
use crate::{
    HandleRegistry, jni_name, lock_or_recover, parsers::string_array_to_vec, run_jni_entry,
};

const ANDROID_PLUGIN_TARGET: &str = "aarch64-linux-android";
const ANDROID_PLUGIN_ARCHITECTURE: &str = "arm64-v8a";
const MAX_PLUGIN_REFERENCE_SET_BYTES: usize = 1024 * 1024;

type AndroidPluginRegistry = Arc<PluginRegistry>;

static PLUGIN_REGISTRIES: OnceLock<Mutex<HandleRegistry<AndroidPluginRegistry>>> = OnceLock::new();

fn plugin_registries() -> &'static Mutex<HandleRegistry<AndroidPluginRegistry>> {
    PLUGIN_REGISTRIES.get_or_init(|| Mutex::new(HandleRegistry::default()))
}

fn invalid_plugin_registry_handle_error() -> &'static str {
    "invalid android plugin registry handle"
}

pub(crate) fn parse_plugin_references(json: &str) -> Result<Vec<PluginReference>, String> {
    if json.len() > MAX_PLUGIN_REFERENCE_SET_BYTES {
        return Err(format!(
            "plugin reference set is {} bytes; maximum is {} bytes",
            json.len(),
            MAX_PLUGIN_REFERENCE_SET_BYTES,
        ));
    }
    let references = serde_json::from_str::<Vec<PluginReference>>(json)
        .map_err(|error| format!("invalid plugin reference JSON: {error}"))?;
    if references.len() > MAX_EMBEDDED_PLUGIN_ARTIFACTS {
        return Err(format!(
            "plugin reference set contains {} entries; maximum is {}",
            references.len(),
            MAX_EMBEDDED_PLUGIN_ARTIFACTS,
        ));
    }
    Ok(references)
}

fn build_android_plugin_registry(
    fragments: &[Vec<u8>],
    references: &[PluginReference],
    native_library_dir: &Path,
    package_paths: &[PathBuf],
    runtime_api_level: u32,
) -> Result<AndroidPluginRegistry, String> {
    let embedded = EmbeddedPluginRegistry::parse_fragments(
        fragments.iter().map(Vec::as_slice),
        ANDROID_PLUGIN_TARGET,
        ANDROID_PLUGIN_ARCHITECTURE,
    )
    .map_err(|error| error.to_string())?;
    let minimum_api_level = parse_android_minimum_api_level(&embedded)?;
    let registry = embedded
        .load_native_selected(references, |locator| match locator {
            EmbeddedPluginLocator::AndroidNativeLibrary { name } => {
                validate_android_runtime_api_level(minimum_api_level, runtime_api_level)?;
                resolve_android_native_library(
                    native_library_dir,
                    package_paths,
                    ANDROID_PLUGIN_ARCHITECTURE,
                    name,
                )
            }
            EmbeddedPluginLocator::AppleFramework { .. } => {
                Err("Apple framework locator is invalid for Android".to_owned())
            }
        })
        .map_err(|error| error.to_string())?;
    Ok(Arc::new(registry))
}

fn parse_android_minimum_api_level(
    registry: &EmbeddedPluginRegistry,
) -> Result<Option<u32>, String> {
    let Some(minimum_os) = registry.minimum_os() else {
        return if registry.artifacts().is_empty() {
            Ok(None)
        } else {
            Err("non-empty Android plugin registry must declare minimum_os".to_owned())
        };
    };
    if !minimum_os.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(format!(
            "Android plugin registry minimum_os `{minimum_os}` must be an unsigned decimal API level"
        ));
    }
    minimum_os.parse::<u32>().map(Some).map_err(|error| {
        format!("invalid Android plugin registry minimum_os `{minimum_os}`: {error}")
    })
}

fn validate_android_runtime_api_level(
    minimum_api_level: Option<u32>,
    runtime_api_level: u32,
) -> Result<(), String> {
    let Some(minimum_api_level) = minimum_api_level else {
        return Ok(());
    };
    if runtime_api_level < minimum_api_level {
        return Err(format!(
            "Android plugin registry requires API level {minimum_api_level}, but the runtime API level is {runtime_api_level}"
        ));
    }
    Ok(())
}

fn runtime_api_level_from_jint(runtime_api_level: jint) -> Result<u32, String> {
    u32::try_from(runtime_api_level)
        .map_err(|_| format!("Android runtime API level must not be negative: {runtime_api_level}"))
}

fn register_android_plugin_registry(registry: AndroidPluginRegistry) -> Result<jlong, String> {
    let mut guard = lock_or_recover(plugin_registries());
    let handle = guard.insert(registry);
    if handle == 0 {
        return Err("android plugin registry overflow".to_owned());
    }
    Ok(handle)
}

pub(crate) fn clone_android_plugin_registry(
    handle: jlong,
) -> Result<AndroidPluginRegistry, &'static str> {
    let guard = lock_or_recover(plugin_registries());
    guard
        .get(handle)
        .cloned()
        .ok_or_else(invalid_plugin_registry_handle_error)
}

fn dispose_android_plugin_registry(handle: jlong) {
    let registry = {
        let mut guard = lock_or_recover(plugin_registries());
        guard.remove(handle)
    };
    drop(registry);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_umbrella22_vesper_player_android_VesperNativeJni_createEmbeddedPluginRegistry(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    registry_fragments: JObjectArray<'_>,
    references_json: JString<'_>,
    native_library_dir: JString<'_>,
    package_paths: JObjectArray<'_>,
    runtime_api_level: jint,
) -> jlong {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<jlong> {
                let fragments = string_array_to_vec(env, registry_fragments)?
                    .into_iter()
                    .map(String::into_bytes)
                    .collect::<Vec<_>>();
                let references_json = references_json.try_to_string(env)?;
                let native_library_dir = PathBuf::from(native_library_dir.try_to_string(env)?);
                if !package_paths.is_null() && package_paths.len(env)? > MAX_ANDROID_PACKAGE_PATHS {
                    env.throw_new(
                        jni_name("java/lang/IllegalArgumentException"),
                        jni_name(format!(
                            "Android package path count exceeds {MAX_ANDROID_PACKAGE_PATHS}"
                        )),
                    )?;
                    return Ok(0);
                }
                let package_paths = string_array_to_vec(env, package_paths)?
                    .into_iter()
                    .map(PathBuf::from)
                    .collect::<Vec<_>>();
                let references = match parse_plugin_references(&references_json) {
                    Ok(references) => references,
                    Err(message) => {
                        env.throw_new(
                            jni_name("java/lang/IllegalArgumentException"),
                            jni_name(message),
                        )?;
                        return Ok(0);
                    }
                };
                let runtime_api_level = match runtime_api_level_from_jint(runtime_api_level) {
                    Ok(runtime_api_level) => runtime_api_level,
                    Err(message) => {
                        env.throw_new(
                            jni_name("java/lang/IllegalArgumentException"),
                            jni_name(message),
                        )?;
                        return Ok(0);
                    }
                };
                // Parsing, hashing, dlopen, and Root ABI validation all run
                // before the global handle registry lock is acquired.
                let registry = match build_android_plugin_registry(
                    &fragments,
                    &references,
                    &native_library_dir,
                    &package_paths,
                    runtime_api_level,
                ) {
                    Ok(registry) => registry,
                    Err(message) => {
                        env.throw_new(
                            jni_name("java/lang/IllegalStateException"),
                            jni_name(message),
                        )?;
                        return Ok(0);
                    }
                };
                match register_android_plugin_registry(registry) {
                    Ok(handle) => Ok(handle),
                    Err(message) => {
                        env.throw_new(
                            jni_name("java/lang/IllegalStateException"),
                            jni_name(message),
                        )?;
                        Ok(0)
                    }
                }
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_umbrella22_vesper_player_android_VesperNativeJni_disposeEmbeddedPluginRegistry(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    registry_handle: jlong,
) {
    run_jni_entry(&mut unowned_env, |_| {
        dispose_android_plugin_registry(registry_handle);
    });
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_umbrella22_vesper_player_android_VesperNativeJni_createBenchmarkSinkSessionFromRegistry(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    registry_handle: jlong,
    references_json: JString<'_>,
) -> jlong {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<jlong> {
                let references_json = references_json.try_to_string(env)?;
                let references = match parse_plugin_references(&references_json) {
                    Ok(references) => references,
                    Err(message) => {
                        env.throw_new(
                            jni_name("java/lang/IllegalArgumentException"),
                            jni_name(message),
                        )?;
                        return Ok(0);
                    }
                };
                let registry = match clone_android_plugin_registry(registry_handle) {
                    Ok(registry) => registry,
                    Err(message) => {
                        env.throw_new(
                            jni_name("java/lang/IllegalArgumentException"),
                            jni_name(message),
                        )?;
                        return Ok(0);
                    }
                };
                match new_benchmark_sink_session_from_registry(registry, references) {
                    Ok(handle) => Ok(handle),
                    Err(message) => {
                        env.throw_new(
                            jni_name("java/lang/IllegalStateException"),
                            jni_name(message),
                        )?;
                        Ok(0)
                    }
                }
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_registry_handle_is_generation_safe_and_arc_outlives_dispose() {
        let registry =
            build_android_plugin_registry(&[], &[], Path::new(""), &[], 0).expect("empty registry");
        let first = register_android_plugin_registry(registry).expect("first handle");
        let retained = clone_android_plugin_registry(first).expect("clone registry");

        dispose_android_plugin_registry(first);
        assert_eq!(
            clone_android_plugin_registry(first).expect_err("stale handle"),
            invalid_plugin_registry_handle_error(),
        );
        assert_eq!(retained.registered_interfaces().len(), 0);

        let second = register_android_plugin_registry(
            build_android_plugin_registry(&[], &[], Path::new(""), &[], 0)
                .expect("second empty registry"),
        )
        .expect("second handle");
        assert_ne!(first, second);
        dispose_android_plugin_registry(second);
    }

    #[test]
    fn invalid_reference_json_is_rejected_before_registry_registration() {
        let error = parse_plugin_references(r#"[{"pluginId":"invalid","transport":"native"}]"#)
            .expect_err("invalid reference");

        assert!(error.contains("reverse-DNS"));
    }

    #[test]
    fn mobile_wasm_reference_is_rejected_without_transport_fallback() {
        let references =
            parse_plugin_references(r#"[{"pluginId":"dev.vesper.fixture","transport":"wasm"}]"#)
                .expect("valid reference wire");
        let fragment = br#"{
            "schema_version":1,
            "target":"aarch64-linux-android",
            "architecture":"arm64-v8a",
            "minimum_os":"26",
            "artifacts":[]
        }"#
        .to_vec();

        let error = build_android_plugin_registry(&[fragment], &references, Path::new(""), &[], 26)
            .expect_err("mobile WASM must be rejected");
        assert!(error.contains("unsupported mobile transport"));
    }

    #[test]
    fn non_empty_registry_requires_compatible_numeric_android_api_level() {
        let references = native_fixture_references();

        let below_minimum = build_android_plugin_registry(
            &[native_fixture_fragment(Some("26"))],
            &references,
            Path::new(""),
            &[],
            25,
        )
        .expect_err("runtime below minimum OS");
        assert!(below_minimum.contains("requires API level 26"));

        let missing = build_android_plugin_registry(
            &[native_fixture_fragment(None)],
            &references,
            Path::new(""),
            &[],
            26,
        )
        .expect_err("missing minimum OS");
        assert!(missing.contains("minimum_os"));
        assert!(missing.contains("required when artifacts are present"));

        let non_numeric = build_android_plugin_registry(
            &[native_fixture_fragment(Some("26.0"))],
            &references,
            Path::new(""),
            &[],
            26,
        )
        .expect_err("non-numeric minimum OS");
        assert!(non_numeric.contains("unsigned decimal API level"));
    }

    #[test]
    fn unreferenced_plugin_does_not_raise_the_no_plugin_runtime_floor() {
        let registry = build_android_plugin_registry(
            &[native_fixture_fragment(Some("30"))],
            &[],
            Path::new(""),
            &[],
            26,
        )
        .expect("unselected packaged plugin");

        assert!(registry.registered_interfaces().is_empty());
    }

    #[test]
    fn android_registry_uses_extracted_or_package_entry_resolver() {
        let references = native_fixture_references();
        let package_paths = [PathBuf::from("missing-base.apk")];
        let error = build_android_plugin_registry(
            &[native_fixture_fragment(Some("26"))],
            &references,
            Path::new(""),
            &package_paths,
            26,
        )
        .expect_err("missing packaged native library");

        assert!(error.contains("not found as an extracted file or package entry"));
    }

    #[test]
    fn negative_jni_runtime_api_level_is_rejected() {
        assert!(runtime_api_level_from_jint(-1).is_err());
        assert_eq!(runtime_api_level_from_jint(26), Ok(26));
    }

    fn native_fixture_references() -> Vec<PluginReference> {
        parse_plugin_references(r#"[{"pluginId":"dev.vesper.fixture","transport":"native"}]"#)
            .expect("valid native fixture reference")
    }

    fn native_fixture_fragment(minimum_os: Option<&str>) -> Vec<u8> {
        let minimum_os = minimum_os
            .map(|value| format!(r#", "minimum_os":"{value}""#))
            .unwrap_or_default();
        format!(
            r#"{{
                "schema_version":1,
                "target":"aarch64-linux-android",
                "architecture":"arm64-v8a"{minimum_os},
                "artifacts":[{{
                    "plugin_id":"dev.vesper.fixture",
                    "transport":"native",
                    "locator":{{
                        "kind":"android-native-library",
                        "name":"vesper_fixture"
                    }},
                    "integrity":{{
                        "kind":"sha256",
                        "digest":"{digest}"
                    }},
                    "package":{{
                        "version":"1.0.0",
                        "publisher":"dev.vesper.publisher",
                        "descriptor_sha256":"{digest}"
                    }},
                    "capabilities":[{{
                        "interface_id":"2d8e5be8-b1de-5e83-8fe0-6118aabc5118",
                        "instance_id":"dev.vesper.fixture.benchmark",
                        "interface_major":1,
                        "interface_minor":0
                    }}]
                }}]
            }}"#,
            digest = "0".repeat(64),
        )
        .into_bytes()
    }
}
