package io.github.ikaros.vesper.player.android

import java.io.ByteArrayInputStream
import org.json.JSONArray
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Test

class VesperEmbeddedPluginRegistryTest {
    @Test
    fun referencesEncodeExplicitTransportAndOptionalInstance() {
        val encoded =
            JSONArray(
                encodeVesperPluginReferences(
                    listOf(
                        VesperPluginReference(
                            pluginId = "dev.vesper.native-plugin",
                            capabilityInstanceId = "dev.vesper.native-plugin.primary",
                            transport = VesperPluginTransport.Native,
                        ),
                        VesperPluginReference.fromWire(
                            pluginId = "dev.vesper.future-plugin",
                            capabilityInstanceId = null,
                            transportRawValue = "future-transport",
                        ),
                    )
                )
            )

        assertEquals("dev.vesper.native-plugin", encoded.getJSONObject(0).getString("pluginId"))
        assertEquals(
            "dev.vesper.native-plugin.primary",
            encoded.getJSONObject(0).getString("capabilityInstanceId"),
        )
        assertEquals("native", encoded.getJSONObject(0).getString("transport"))
        assertEquals("future-transport", encoded.getJSONObject(1).getString("transport"))
        assertEquals(false, encoded.getJSONObject(1).has("capabilityInstanceId"))
    }

    @Test
    fun fragmentDiscoveryUsesStableSortedAssetPaths() {
        val openedPaths = mutableListOf<String>()
        val fragments =
            loadVesperPluginRegistryFragments(
                architecture = "arm64-v8a",
                listAssets = {
                    listOf("dev.vesper.second.json", "dev.vesper.first.json")
                },
                openAsset = { path ->
                    openedPaths += path
                    ByteArrayInputStream(path.encodeToByteArray())
                },
            )

        assertEquals(
            listOf(
                "vesper/plugins/arm64-v8a/dev.vesper.first.json",
                "vesper/plugins/arm64-v8a/dev.vesper.second.json",
            ),
            openedPaths,
        )
        assertEquals(openedPaths, fragments.asList())
    }

    @Test
    fun fragmentDiscoveryRejectsOversizedAndInvalidAssets() {
        assertThrows(IllegalArgumentException::class.java) {
            loadVesperPluginRegistryFragments(
                architecture = "arm64-v8a",
                listAssets = { listOf("not-a-plugin.json") },
                openAsset = { ByteArrayInputStream(byteArrayOf()) },
            )
        }
        assertThrows(IllegalArgumentException::class.java) {
            loadVesperPluginRegistryFragments(
                architecture = "arm64-v8a",
                listAssets = { listOf("dev.vesper.plugin.json") },
                openAsset = {
                    ByteArrayInputStream(ByteArray(1024 * 1024 + 1))
                },
            )
        }
    }

    @Test
    fun architectureSelectionDoesNotFallbackToAnotherAbi() {
        assertEquals(
            "arm64-v8a",
            selectVesperPluginArchitecture(listOf("x86_64", "arm64-v8a")),
        )
        assertThrows(IllegalArgumentException::class.java) {
            selectVesperPluginArchitecture(listOf("x86_64"))
        }
    }

    @Test
    fun packagePathsAreBaseFirstExactAndBounded() {
        val paths =
            collectVesperAndroidPackagePaths(
                basePackagePath = " /data/app/base.apk ",
                splitPackagePaths =
                    listOf(
                        "",
                        "/data/app/split_config.arm64_v8a.apk",
                        " /data/app/base.apk ",
                        "/data/app/插件.apk",
                    ),
            )

        assertEquals(
            listOf(
                " /data/app/base.apk ",
                "/data/app/split_config.arm64_v8a.apk",
                "/data/app/插件.apk",
            ),
            paths.asList(),
        )
        assertEquals(
            listOf("/same.apk"),
            collectVesperAndroidPackagePaths(
                    basePackagePath = "/same.apk",
                    splitPackagePaths = List(MAX_ANDROID_PACKAGE_PATHS + 1) { "/same.apk" },
                )
                .asList(),
        )
    }

    @Test
    fun packagePathsRejectTheFirstUniqueEntryPastTheLimit() {
        val maximumPaths =
            (0 until MAX_ANDROID_PACKAGE_PATHS).map { index -> "/data/app/split-$index.apk" }
        assertEquals(
            MAX_ANDROID_PACKAGE_PATHS,
            collectVesperAndroidPackagePaths(null, maximumPaths).size,
        )
        assertThrows(IllegalArgumentException::class.java) {
            collectVesperAndroidPackagePaths(
                null,
                maximumPaths + "/data/app/one-too-many.apk",
            )
        }
        assertEquals(0, collectVesperAndroidPackagePaths(null, emptyList()).size)
    }
}
