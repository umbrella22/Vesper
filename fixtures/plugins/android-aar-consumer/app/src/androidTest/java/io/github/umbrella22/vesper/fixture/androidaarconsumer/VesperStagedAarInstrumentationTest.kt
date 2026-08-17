package io.github.umbrella22.vesper.fixture.androidaarconsumer

import android.content.Context
import android.net.Uri
import android.os.SystemClock
import androidx.test.core.app.ActivityScenario
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import dalvik.system.BaseDexClassLoader
import io.github.umbrella22.vesper.player.android.VesperBundledPluginReferences
import io.github.umbrella22.vesper.player.android.VesperFrameProcessorConfiguration
import io.github.umbrella22.vesper.player.android.VesperFrameProcessorMode
import io.github.umbrella22.vesper.player.android.VesperNativeFramePipelineConfiguration
import io.github.umbrella22.vesper.player.android.VesperNativeFramePipelineMode
import io.github.umbrella22.vesper.player.android.VesperPlayerController
import io.github.umbrella22.vesper.player.android.VesperPlayerControllerFactory
import io.github.umbrella22.vesper.player.android.VesperPlayerSource
import io.github.umbrella22.vesper.player.android.VesperSourceNormalizerConfiguration
import io.github.umbrella22.vesper.player.android.VesperSourceNormalizerMode
import java.io.File
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class VesperStagedAarInstrumentationTest {
    @Test
    fun rawAarsPackageRegistryFragmentsAndNativeLibraries() {
        val context = ApplicationProvider.getApplicationContext<Context>()
        val registryFiles =
            context.assets
                .list("vesper/plugins/arm64-v8a")
                .orEmpty()
                .toSet()

        assertEquals(
            setOf(
                "dev.vesper.frame-processor-diagnostic.json",
                "io.github.umbrella22.vesper.decoder-mediacodec.json",
                "io.github.umbrella22.vesper.remux-ffmpeg.json",
                "io.github.umbrella22.vesper.source-normalizer-ffmpeg.json",
            ),
            registryFiles,
        )

        listOf(
            "vesper_player_android",
            "vesper_source_normalizer_ffmpeg",
            "vesper_remux_ffmpeg",
            "vesper_decoder_mediacodec",
            "vesper_frame_processor_diagnostic",
            "avcodec",
            "avformat",
            "avutil",
            "xml2",
        ).forEach { libraryName ->
            assertNotNull(
                "missing ${System.mapLibraryName(libraryName)}",
                findPackagedLibrary(context, libraryName),
            )
        }
    }

    @Test
    fun publicPluginReferencesLoadCapabilitiesFromRawAars() = runBlocking {
        val context = ApplicationProvider.getApplicationContext<Context>()
        val fixtureDirectory = prepareMediaFixture(context, "load")
        val controller = createRawAarController(context, fixtureDirectory.resolve(MEDIA_FIXTURE_NAME))

        try {
            controller.initializeAsync()
            val diagnostics = awaitOpenPluginDiagnostics(controller)
            assertDiagnostic(diagnostics, "source_normalizer", "sourceNormalizerSupported")
            assertDiagnostic(diagnostics, "frame_processor", "frameProcessorSupported")
            val nativeFrame =
                diagnostics.single { diagnostic ->
                    diagnostic["pluginKind"] == "native_frame_pipeline"
                }
            assertEquals("loaded", nativeFrame["status"])
            assertEquals("sdkManagedNativeFrame", nativeFrame["route"])
            assertEquals("open", nativeFrame["lifecycle"])
            assertEquals(false, nativeFrame["surfaceAttached"])
            assertEquals("waitingForSurface", nativeFrame["presenterState"])
        } finally {
            controller.dispose()
            fixtureDirectory.deleteRecursively()
        }
    }

    @Test
    fun publicSurfaceRouteExecutesDecoderAndFrameProcessorFromRawAars() {
        val context = ApplicationProvider.getApplicationContext<Context>()
        val fixtureDirectory = prepareMediaFixture(context, "execute")
        val scenario = ActivityScenario.launch(VesperSurfaceHostActivity::class.java)
        var controller: VesperPlayerController? = null

        try {
            scenario.onActivity { activity ->
                controller =
                    createRawAarController(
                        activity,
                        fixtureDirectory.resolve(MEDIA_FIXTURE_NAME),
                    ).also { created ->
                        created.attachSurfaceHost(activity.surfaceHost)
                    }
            }
            val activeController = checkNotNull(controller)
            runBlocking { activeController.initializeAsync() }
            scenario.onActivity { activeController.play() }

            val diagnostics = awaitPresentedPluginDiagnostics(activeController)
            assertDiagnostic(diagnostics, "source_normalizer", "sourceNormalizerSupported")
            assertDiagnostic(diagnostics, "frame_processor", "frameProcessorSupported")
            val nativeFrame =
                diagnostics.single { diagnostic ->
                    diagnostic["pluginKind"] == "native_frame_pipeline"
                }
            assertEquals("open", nativeFrame["lifecycle"])
            assertEquals(true, nativeFrame["surfaceAttached"])
            assertEquals(true, nativeFrame["presenterReady"])
            assertTrue((nativeFrame["decodedFrames"] as Number).toLong() > 0L)
            assertTrue((nativeFrame["processedFrames"] as Number).toLong() > 0L)
            assertTrue((nativeFrame["presentedFrames"] as Number).toLong() > 0L)
        } finally {
            controller?.let { activeController ->
                scenario.onActivity { activeController.dispose() }
            }
            scenario.close()
            fixtureDirectory.deleteRecursively()
        }
    }
}

private const val MEDIA_FIXTURE_NAME = "tiny-h264-aac-mediacodec.m4v"

private fun prepareMediaFixture(
    context: Context,
    suffix: String,
): File {
    val fixtureDirectory = File(context.cacheDir, "vesper-staged-aar-consumer-$suffix")
    fixtureDirectory.deleteRecursively()
    check(fixtureDirectory.mkdirs()) { "failed to create media fixture directory" }
    InstrumentationRegistry.getInstrumentation().context.assets
        .open(MEDIA_FIXTURE_NAME)
        .use { input ->
            fixtureDirectory.resolve(MEDIA_FIXTURE_NAME).outputStream().use(input::copyTo)
        }
    return fixtureDirectory
}

private fun createRawAarController(
    context: Context,
    mediaFile: File,
): VesperPlayerController =
    VesperPlayerControllerFactory.createDefault(
        context = context,
        initialSource =
            VesperPlayerSource.local(
                uri = Uri.fromFile(mediaFile).toString(),
                label = mediaFile.name,
            ),
        sourceNormalizerConfiguration =
            VesperSourceNormalizerConfiguration(
                mode = VesperSourceNormalizerMode.PreflightOnly,
                pluginReferences =
                    listOf(VesperBundledPluginReferences.sourceNormalizerFfmpeg),
            ),
        frameProcessorConfiguration =
            VesperFrameProcessorConfiguration(
                mode = VesperFrameProcessorMode.DiagnosticsOnly,
                pluginReferences =
                    listOf(VesperBundledPluginReferences.frameProcessorDiagnostic),
            ),
        nativeFramePipelineConfiguration =
            VesperNativeFramePipelineConfiguration(
                mode = VesperNativeFramePipelineMode.RequireNativeFrame,
                decoderPluginReferences =
                    listOf(VesperBundledPluginReferences.decoderMediaCodec),
                frameProcessorPluginReferences =
                    listOf(VesperBundledPluginReferences.frameProcessorDiagnostic),
                maxInFlightFrames = 3,
            ),
    )

private fun findPackagedLibrary(
    context: Context,
    libraryName: String,
): String? =
    (context.classLoader as? BaseDexClassLoader)
        ?.findLibrary(libraryName)
        ?.takeIf { path -> path.isNotBlank() && File(path).isFile }

private fun awaitOpenPluginDiagnostics(
    controller: VesperPlayerController,
): List<Map<String, Any?>> {
    val deadlineMs = SystemClock.elapsedRealtime() + 20_000L
    var latest = readPluginDiagnosticsOnMain(controller)
    while (SystemClock.elapsedRealtime() < deadlineMs) {
        latest = readPluginDiagnosticsOnMain(controller)
        val nativeFrame = latest.firstOrNull { it["pluginKind"] == "native_frame_pipeline" }
        if (nativeFrame?.get("lifecycle") == "open") {
            return latest
        }
        if (nativeFrame?.get("lifecycle") == "failed") {
            error("native-frame pipeline failed: $nativeFrame")
        }
        SystemClock.sleep(10L)
    }
    error("native-frame pipeline did not open within 20 seconds: $latest")
}

private fun awaitPresentedPluginDiagnostics(
    controller: VesperPlayerController,
): List<Map<String, Any?>> {
    val deadlineMs = SystemClock.elapsedRealtime() + 20_000L
    var latest = readPluginDiagnosticsOnMain(controller)
    while (SystemClock.elapsedRealtime() < deadlineMs) {
        latest = readPluginDiagnosticsOnMain(controller)
        val nativeFrame = latest.firstOrNull { it["pluginKind"] == "native_frame_pipeline" }
        if (nativeFrame?.get("lifecycle") == "failed") {
            error("native-frame pipeline failed: $nativeFrame")
        }
        if (
            nativeFrame?.get("lifecycle") == "open" &&
                nativeFrame["surfaceAttached"] == true &&
                (nativeFrame["decodedFrames"] as? Number)?.toLong()?.let { it > 0L } == true &&
                (nativeFrame["processedFrames"] as? Number)?.toLong()?.let { it > 0L } == true &&
                (nativeFrame["presentedFrames"] as? Number)?.toLong()?.let { it > 0L } == true
        ) {
            return latest
        }
        SystemClock.sleep(10L)
    }
    error("native-frame pipeline did not present a processed frame within 20 seconds: $latest")
}

private fun readPluginDiagnosticsOnMain(
    controller: VesperPlayerController,
): List<Map<String, Any?>> {
    var diagnostics = emptyList<Map<String, Any?>>()
    InstrumentationRegistry.getInstrumentation().runOnMainSync {
        diagnostics = controller.pluginDiagnostics
    }
    return diagnostics
}

private fun assertDiagnostic(
    diagnostics: List<Map<String, Any?>>,
    pluginKind: String,
    status: String,
) {
    assertTrue(
        "missing $pluginKind/$status diagnostic: $diagnostics",
        diagnostics.any { diagnostic ->
            diagnostic["pluginKind"] == pluginKind && diagnostic["status"] == status
        },
    )
}
