package io.github.ikaros.vesper.player.android

import android.content.Context
import android.content.res.AssetManager
import android.os.Build
import java.io.ByteArrayOutputStream
import java.io.InputStream
import java.nio.ByteBuffer
import java.nio.charset.CodingErrorAction
import java.util.concurrent.atomic.AtomicLong
import org.json.JSONArray
import org.json.JSONObject

private const val EMBEDDED_PLUGIN_ASSET_ROOT = "vesper/plugins"
private const val ANDROID_PLUGIN_ARCHITECTURE = "arm64-v8a"
private const val MAX_REGISTRY_FRAGMENT_BYTES = 1024 * 1024
private const val MAX_REGISTRY_SET_BYTES = 4 * 1024 * 1024
private const val MAX_REGISTRY_FRAGMENTS = 256
private const val MAX_PLUGIN_REFERENCES = 256
internal const val MAX_ANDROID_PACKAGE_PATHS = 256
private const val READ_BUFFER_BYTES = 8 * 1024

/** Owns one generation-safe Rust plugin registry handle. */
internal interface VesperPluginRegistryHandleOwner : AutoCloseable {
    val handle: Long
}

internal fun interface VesperPluginRegistryFactory {
    fun create(
        context: Context?,
        references: List<VesperPluginReference>,
    ): VesperPluginRegistryHandleOwner
}

internal object DefaultVesperPluginRegistryFactory : VesperPluginRegistryFactory {
    override fun create(
        context: Context?,
        references: List<VesperPluginReference>,
    ): VesperPluginRegistryHandleOwner =
        VesperEmbeddedPluginRegistry.create(
            requireNotNull(context) {
                "Android Context is required when plugin references are configured"
            },
            references,
        )
}

internal class VesperEmbeddedPluginRegistry private constructor(
    handle: Long,
) : VesperPluginRegistryHandleOwner {
    private val handleState = AtomicLong(handle)

    override val handle: Long
        get() = handleState.get()

    override fun close() {
        val handle = handleState.getAndSet(0L)
        if (handle != 0L) {
            VesperNativeJni.disposeEmbeddedPluginRegistry(handle)
        }
    }

    companion object {
        /**
         * Discovers packaged fragments and loads only explicitly referenced plugins.
         *
         * The caller must keep hashing and dynamic loading off the main thread.
         */
        internal fun create(
            context: Context,
            references: List<VesperPluginReference>,
        ): VesperEmbeddedPluginRegistry {
            require(references.size <= MAX_PLUGIN_REFERENCES) {
                "plugin reference count exceeds $MAX_PLUGIN_REFERENCES"
            }
            val architecture = selectVesperPluginArchitecture(Build.SUPPORTED_ABIS.asList())
            val fragments =
                loadVesperPluginRegistryFragments(
                    architecture = architecture,
                    listAssets = { path -> context.assets.list(path).orEmpty().asList() },
                    openAsset = { path -> context.assets.open(path, AssetManager.ACCESS_STREAMING) },
                )
            val applicationInfo = context.applicationInfo
            val nativeLibraryDir = applicationInfo.nativeLibraryDir.orEmpty()
            val packagePaths =
                collectVesperAndroidPackagePaths(
                    basePackagePath = applicationInfo.sourceDir,
                    splitPackagePaths = applicationInfo.splitSourceDirs?.asList().orEmpty(),
                )
            require(
                references.isEmpty() ||
                    nativeLibraryDir.isNotEmpty() ||
                    packagePaths.isNotEmpty()
            ) {
                "Android native library directory and package paths are unavailable"
            }
            VesperNativeLibrary.ensureLoaded()
            val handle =
                VesperNativeJni.createEmbeddedPluginRegistry(
                    registryFragments = fragments,
                    referencesJson = encodeVesperPluginReferences(references),
                    nativeLibraryDir = nativeLibraryDir,
                    packagePaths = packagePaths,
                    runtimeApiLevel = Build.VERSION.SDK_INT,
                )
            check(handle != 0L) { "native plugin registry handle must not be zero" }
            return VesperEmbeddedPluginRegistry(handle)
        }
    }
}

internal fun collectVesperAndroidPackagePaths(
    basePackagePath: String?,
    splitPackagePaths: List<String?>,
): Array<String> {
    val packagePaths = linkedSetOf<String>()
    sequenceOf(basePackagePath)
        .plus(splitPackagePaths.asSequence())
        .filterNotNull()
        .filter(String::isNotEmpty)
        .forEach { path ->
            if (path !in packagePaths) {
                require(packagePaths.size < MAX_ANDROID_PACKAGE_PATHS) {
                    "Android package path count exceeds $MAX_ANDROID_PACKAGE_PATHS"
                }
                packagePaths += path
            }
        }
    return packagePaths.toTypedArray()
}

internal fun selectVesperPluginArchitecture(supportedAbis: List<String>): String {
    require(ANDROID_PLUGIN_ARCHITECTURE in supportedAbis) {
        "Vesper embedded plugins require Android arm64-v8a"
    }
    return ANDROID_PLUGIN_ARCHITECTURE
}

internal fun encodeVesperPluginReferences(references: List<VesperPluginReference>): String {
    require(references.size <= MAX_PLUGIN_REFERENCES) {
        "plugin reference count exceeds $MAX_PLUGIN_REFERENCES"
    }
    val encoded = JSONArray()
    references.forEach { reference ->
        encoded.put(reference.toJsonObject())
    }
    return encoded.toString()
}

internal fun encodeVesperResolvedMobilePluginArtifacts(
    artifacts: List<VesperResolvedMobilePluginArtifact>,
): String {
    require(artifacts.size <= MAX_PLUGIN_REFERENCES) {
        "mobile plugin artifact count exceeds $MAX_PLUGIN_REFERENCES"
    }
    val encoded = JSONArray()
    artifacts.forEach { artifact ->
        encoded.put(
            JSONObject()
                .put("reference", artifact.reference.toJsonObject())
                .put("libraryPath", artifact.libraryPath),
        )
    }
    return encoded.toString()
}

private fun VesperPluginReference.toJsonObject(): JSONObject {
    val encoded =
        JSONObject()
            .put("pluginId", pluginId)
            .put("transport", transportWireName)
    capabilityInstanceId?.let { instanceId ->
        encoded.put("capabilityInstanceId", instanceId)
    }
    return encoded
}

internal fun loadVesperPluginRegistryFragments(
    architecture: String,
    listAssets: (String) -> List<String>,
    openAsset: (String) -> InputStream,
): Array<String> {
    require(architecture == ANDROID_PLUGIN_ARCHITECTURE) {
        "unsupported Vesper plugin architecture: $architecture"
    }
    val assetDirectory = "$EMBEDDED_PLUGIN_ASSET_ROOT/$architecture"
    val fileNames = listAssets(assetDirectory).sorted()
    require(fileNames.size <= MAX_REGISTRY_FRAGMENTS) {
        "plugin registry fragment count exceeds $MAX_REGISTRY_FRAGMENTS"
    }
    require(fileNames.distinct().size == fileNames.size) {
        "plugin registry contains duplicate asset names"
    }

    var totalBytes = 0
    return fileNames.map { fileName ->
        require(fileName.endsWith(".json")) {
            "unexpected plugin registry asset: $fileName"
        }
        val pluginId = fileName.removeSuffix(".json")
        require(isValidPluginIdentity(pluginId)) {
            "plugin registry asset name must be a reverse-DNS plugin id: $fileName"
        }
        val bytes =
            openAsset("$assetDirectory/$fileName").use { input ->
                readBounded(input, MAX_REGISTRY_FRAGMENT_BYTES)
            }
        totalBytes += bytes.size
        require(totalBytes <= MAX_REGISTRY_SET_BYTES) {
            "plugin registry fragments exceed $MAX_REGISTRY_SET_BYTES bytes"
        }
        decodeStrictUtf8(bytes, fileName)
    }.toTypedArray()
}

private fun readBounded(input: InputStream, maximumBytes: Int): ByteArray {
    val output = ByteArrayOutputStream(minOf(maximumBytes, READ_BUFFER_BYTES))
    val buffer = ByteArray(READ_BUFFER_BYTES)
    var totalBytes = 0
    while (true) {
        val remainingWithSentinel = maximumBytes - totalBytes + 1
        val count = input.read(buffer, 0, minOf(buffer.size, remainingWithSentinel))
        if (count < 0) {
            return output.toByteArray()
        }
        totalBytes += count
        require(totalBytes <= maximumBytes) {
            "plugin registry fragment exceeds $maximumBytes bytes"
        }
        output.write(buffer, 0, count)
    }
}

private fun decodeStrictUtf8(bytes: ByteArray, fileName: String): String =
    try {
        Charsets.UTF_8
            .newDecoder()
            .onMalformedInput(CodingErrorAction.REPORT)
            .onUnmappableCharacter(CodingErrorAction.REPORT)
            .decode(ByteBuffer.wrap(bytes))
            .toString()
    } catch (error: CharacterCodingException) {
        throw IllegalArgumentException("plugin registry asset is not UTF-8: $fileName", error)
    }
