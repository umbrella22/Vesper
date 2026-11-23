package io.github.umbrella22.vesper.player.android

import android.content.Context
import android.content.pm.PackageManager
import android.graphics.SurfaceTexture
import android.net.Uri
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.view.Surface
import androidx.test.core.app.ActivityScenario
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import dalvik.system.BaseDexClassLoader
import java.io.File
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeout
import org.junit.Assume.assumeFalse
import org.junit.Assume.assumeTrue
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class VesperNativeFramePipelineInstrumentationTest {
    @Test
    fun realMediaCodecPipelineAttachesReattachesAndDrainsOutstandingFrameOnClose() {
        val context = ApplicationProvider.getApplicationContext<Context>()
        assumeTrue("The device does not support Vulkan 1.1", supportsVulkan11(context))
        val fixtureDirectory = newFixtureDirectory(context)
        val source = mediaSource(fixtureDirectory)
        val sourceNormalizerConfiguration = sourceNormalizerConfiguration()
        val nativeFrameConfiguration = pipelineConfiguration(VesperNativeFramePipelineMode.RequireNativeFrame)
        val bindings = nativeFrameBindings(context)
        val surfaceTexture = SurfaceTexture(0)
        surfaceTexture.setDefaultBufferSize(128, 96)
        val presented = CountDownLatch(1)
        surfaceTexture.setOnFrameAvailableListener({ presented.countDown() }, Handler(Looper.getMainLooper()))
        val surface = Surface(surfaceTexture)

        try {
            val firstOpen =
                checkNotNull(
                    bindings.openNativeFramePipeline(
                        source = source,
                        sourceNormalizerConfiguration = sourceNormalizerConfiguration,
                        nativeFramePipelineConfiguration = nativeFrameConfiguration,
                        surfaceKind = NativeVideoSurfaceKind.SurfaceView,
                    ),
                )
            val firstHandle = (firstOpen["handle"] as? Number)?.toLong()
            assertNotNull(firstHandle)
            assertNotEquals(0L, firstHandle)
            assertEquals("sdkManagedNativeFrame", firstOpen["route"])
            assertEquals("sdkOwnedHardwareBuffer", firstOpen["selectedProfile"])

            val attached =
                checkNotNull(
                    bindings.attachNativeFramePipelineSurface(
                        surface,
                        NativeVideoSurfaceKind.SurfaceView,
                    ),
                )
            assertEquals(true, attached["surfaceAttached"])
            assertEquals("ready", attached["presenterState"])

            val detached = checkNotNull(bindings.detachNativeFramePipelineSurface())
            assertEquals(false, detached["surfaceAttached"])
            assertEquals("waitingForSurface", detached["presenterState"])

            val reattached =
                checkNotNull(
                    bindings.attachNativeFramePipelineSurface(
                        surface,
                        NativeVideoSurfaceKind.SurfaceView,
                    ),
                )
            assertEquals(true, reattached["surfaceAttached"])

            val outstandingFrame = awaitNativeFrame(bindings)
            assertEquals(true, outstandingFrame["requiresHostRelease"])
            assertNotEquals(0L, (outstandingFrame["handle"] as? Number)?.toLong())

            bindings.closeNativeFramePipeline()
            assertNull(bindings.nativeFramePipelineHandle)
            assertFalse(bindings.nativeFramePipelineOwnsSurface.get())

            val secondOpen =
                checkNotNull(
                    bindings.openNativeFramePipeline(
                        source = source,
                        sourceNormalizerConfiguration = sourceNormalizerConfiguration,
                        nativeFramePipelineConfiguration = nativeFrameConfiguration,
                        surfaceKind = NativeVideoSurfaceKind.SurfaceView,
                    ),
                )
            val secondHandle = (secondOpen["handle"] as? Number)?.toLong()
            assertNotNull(secondHandle)
            assertNotEquals(firstHandle, secondHandle)
            bindings.attachNativeFramePipelineSurface(
                surface,
                NativeVideoSurfaceKind.SurfaceView,
            )
            val firstReleasableFrame = awaitNativeFrame(bindings)
            val firstFrameHandle =
                checkNotNull((firstReleasableFrame["handle"] as? Number)?.toLong())
            assertNotNull(
                bindings.releaseNativeFramePipelineFrame(firstFrameHandle, presented = false),
            )
            val secondReleasableFrame = awaitNativeFrame(bindings)
            val secondFrameHandle =
                checkNotNull((secondReleasableFrame["handle"] as? Number)?.toLong())
            assertNotEquals(firstFrameHandle, secondFrameHandle)
            assertNotNull(bindings.seekNativeFramePipeline(0L))
            assertThrows(IllegalStateException::class.java) {
                bindings.releaseNativeFramePipelineFrame(secondFrameHandle, presented = true)
            }
            val seekFrame = awaitNativeFrame(bindings)
            val seekFrameHandle = checkNotNull((seekFrame["handle"] as? Number)?.toLong())
            assertNotEquals(secondFrameHandle, seekFrameHandle)
            val presentation = checkNotNull(bindings.releaseNativeFramePipelineFrame(seekFrameHandle, presented = true))
            val counters = checkNotNull(presentation["counters"] as? Map<*, *>)
            assertEquals(1L, (counters["processedFrames"] as? Number)?.toLong())
            assertEquals(1L, (counters["presentedFrames"] as? Number)?.toLong())
            assertTrue("Vulkan submitted a Surface frame", presented.await(3, TimeUnit.SECONDS))
            bindings.closeNativeFramePipeline()
            assertNull(bindings.nativeFramePipelineHandle)
            assertFalse(bindings.nativeFramePipelineOwnsSurface.get())
        } finally {
            bindings.closeNativeFramePipeline()
            bindings.dispose()
            surface.release()
            surfaceTexture.release()
            fixtureDirectory.deleteRecursively()
        }
    }

    @Test
    fun oldVulkanDeviceReportsAdmissionFailureAndKeepsMedia3Playback() = runBlocking {
        val context = ApplicationProvider.getApplicationContext<Context>()
        assumeFalse("This test exercises a device below the Vulkan 1.1 floor", supportsVulkan11(context))
        val directory = newFixtureDirectory(context)
        try {
            val source = mediaSource(directory)
            for (mode in listOf(VesperNativeFramePipelineMode.PreferNativeFrame, VesperNativeFramePipelineMode.RequireNativeFrame)) {
                ActivityScenario.launch(VesperSurfaceLayoutTestActivity::class.java).use { scenario ->
                    lateinit var controller: VesperPlayerController
                    lateinit var bindings: VesperNativeJniBindings
                    scenario.onActivity { activity ->
                        bindings = nativeFrameBindings(context)
                        controller = VesperPlayerController(VesperNativePlayerBridge(
                            bindings = bindings,
                            appContext = context,
                            sourceNormalizerConfiguration = sourceNormalizerConfiguration(),
                            nativeFramePipelineConfiguration = pipelineConfiguration(mode),
                        ))
                        controller.attachSurfaceHost(activity.replaceSurfaceHost())
                    }
                    try {
                        val result = runCatching { controller.selectSourceAsync(source) }
                        withContext(Dispatchers.Main.immediate) {
                            val reason = withTimeout(15_000L) {
                                var published = nativeFrameFailureReason(controller)
                                while (published == null) {
                                    delay(50L)
                                    published = nativeFrameFailureReason(controller)
                                }
                                published
                            }
                            assertTrue("Expected Vulkan admission failure, got $reason", reason.contains("Vulkan"))
                            assertNull(bindings.nativeFramePipelineHandle)
                            assertFalse(bindings.nativeFramePipelineOwnsSurface.get())
                            if (mode == VesperNativeFramePipelineMode.PreferNativeFrame) {
                                result.getOrThrow()
                                controller.play()
                                withTimeout(10_000L) {
                                    while ((controller.sampleTimeline()?.positionMs ?: 0L) < 250L) delay(50L)
                                }
                                assertNull(controller.uiState.value.lastError)
                            } else {
                                val failure = result.exceptionOrNull()
                                assertTrue("Expected a source command failure, got $result", failure is VesperPlayerCommandException)
                                val error = (failure as VesperPlayerCommandException).errorState
                                assertEquals(reason, error.message)
                                assertEquals(VesperPlayerErrorCode.Unsupported, error.code)
                                assertEquals(VesperPlayerErrorCategory.Capability, error.category)
                                assertFalse(error.retriable)
                                assertEquals("nativeFramePipelineOpenFailed", error.details["reason"])
                                controller.refresh()
                                assertEquals(error, controller.uiState.value.lastError)
                            }
                        }
                    } finally {
                        scenario.onActivity { controller.dispose() }
                    }
                }
            }
        } finally {
            directory.deleteRecursively()
        }
        Unit
    }
}

private fun supportsVulkan11(context: Context): Boolean =
    context.packageManager.hasSystemFeature(PackageManager.FEATURE_VULKAN_HARDWARE_VERSION, (1 shl 22) or (1 shl 12))

private fun nativeFrameFailureReason(controller: VesperPlayerController): String? =
    controller.pluginDiagnostics.firstNotNullOfOrNull { entry ->
        if (entry["pluginKind"] == "native_frame_pipeline") entry["fallbackReason"] as? String else null
    }

private fun newFixtureDirectory(context: Context): File =
    kotlin.io.path.createTempDirectory(context.cacheDir.toPath(), "vesper-native-frame-").toFile()

private fun mediaSource(directory: File): VesperPlayerSource {
    val file = File(directory, "video.m4v")
    InstrumentationRegistry.getInstrumentation().context.assets.open("device-720p-h264-aac.m4v").use { input ->
        file.outputStream().use(input::copyTo)
    }
    return VesperPlayerSource.local(Uri.fromFile(file).toString(), "Native-frame AVC fixture")
}

private fun sourceNormalizerConfiguration() = VesperSourceNormalizerConfiguration(
    mode = VesperSourceNormalizerMode.PreflightOnly,
    pluginReferences = listOf(VesperBundledPluginReferences.sourceNormalizerFfmpeg),
)

private fun pipelineConfiguration(mode: VesperNativeFramePipelineMode) = VesperNativeFramePipelineConfiguration(
    mode = mode,
    decoderPluginReferences = listOf(VesperBundledPluginReferences.decoderMediaCodec),
    maxInFlightFrames = 3,
)

private fun nativeFrameBindings(context: Context): VesperNativeJniBindings {
    val instrumentationContext = InstrumentationRegistry.getInstrumentation().context
    val sourceNormalizerPath =
        requireBundledLibrary(
            contexts = listOf(instrumentationContext, context),
            libraryName = "vesper_source_normalizer_ffmpeg",
        )
    val decoderPath =
        requireBundledLibrary(
            contexts = listOf(instrumentationContext, context),
            libraryName = "vesper_decoder_mediacodec",
        )
    return VesperNativeJniBindings(
            context,
            resolvedPluginArtifacts =
                VesperResolvedMobilePluginArtifacts(
                    sourceNormalizerArtifacts =
                        listOf(
                            VesperResolvedMobilePluginArtifact(
                                VesperBundledPluginReferences.sourceNormalizerFfmpeg,
                                sourceNormalizerPath,
                            ),
                        ),
                    decoderArtifacts =
                        listOf(
                            VesperResolvedMobilePluginArtifact(
                                VesperBundledPluginReferences.decoderMediaCodec,
                                decoderPath,
                            ),
                        ),
                ),
        )
}

private fun awaitNativeFrame(bindings: VesperNativeJniBindings): Map<String, Any?> {
    val deadlineMs = SystemClock.elapsedRealtime() + 10_000L
    var lastStatus: Map<String, Any?>? = null
    while (SystemClock.elapsedRealtime() < deadlineMs) {
        lastStatus = bindings.advanceNativeFramePipeline()
        when (lastStatus?.get("status")) {
            "frame" -> return lastStatus
            "error", "failed", "eof" -> error("native-frame pipeline terminated: $lastStatus")
        }
        SystemClock.sleep(2L)
    }
    error("native-frame pipeline did not return a frame within 10 seconds: $lastStatus")
}

private fun requireBundledLibrary(
    contexts: List<Context>,
    libraryName: String,
): String =
    contexts
        .asSequence()
        .mapNotNull { context ->
            (context.classLoader as? BaseDexClassLoader)
                ?.findLibrary(libraryName)
                ?.takeIf { path -> path.isNotBlank() && File(path).isFile }
        }
        .firstOrNull()
        ?: error("instrumentation APK does not contain ${System.mapLibraryName(libraryName)}")
