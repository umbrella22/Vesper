package io.github.ikaros.vesper.player.android

import android.content.Context
import android.graphics.SurfaceTexture
import android.net.Uri
import android.os.SystemClock
import android.view.Surface
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import dalvik.system.BaseDexClassLoader
import java.io.File
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class VesperNativeFramePipelineInstrumentationTest {
    @Test
    fun realMediaCodecPipelineAttachesReattachesAndDrainsOutstandingFrameOnClose() {
        val context = ApplicationProvider.getApplicationContext<Context>()
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
        val fixtureDirectory = File(context.cacheDir, "vesper-native-frame-instrumentation")
        fixtureDirectory.deleteRecursively()
        check(fixtureDirectory.mkdirs()) { "failed to create native-frame fixture directory" }
        val mediaFile = File(fixtureDirectory, "video.m4v")
        context.assets.open("tiny-h264-aac-mediacodec.m4v").use { input ->
            mediaFile.outputStream().use(input::copyTo)
        }
        val source = VesperPlayerSource.local(Uri.fromFile(mediaFile).toString(), "video.m4v")
        val sourceNormalizerConfiguration =
            VesperSourceNormalizerConfiguration(
                mode = VesperSourceNormalizerMode.PreflightOnly,
                pluginReferences = listOf(VesperBundledPluginReferences.sourceNormalizerFfmpeg),
            )
        val nativeFrameConfiguration =
            VesperNativeFramePipelineConfiguration(
                mode = VesperNativeFramePipelineMode.RequireNativeFrame,
                decoderPluginReferences = listOf(VesperBundledPluginReferences.decoderMediaCodec),
                maxInFlightFrames = 3,
            )
        val bindings =
            VesperNativeJniBindings(
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
        val surfaceTexture = SurfaceTexture(0)
        surfaceTexture.setDefaultBufferSize(128, 96)
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
            assertNotNull(
                bindings.releaseNativeFramePipelineFrame(secondFrameHandle, presented = false),
            )
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
