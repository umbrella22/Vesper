package io.github.ikaros.vesper.player.android.external.internal.relay.ffmpeg

import android.content.Context
import android.system.ErrnoException
import android.system.Os
import android.system.OsConstants
import io.github.ikaros.vesper.player.android.VesperPlayerSourceProtocol
import io.github.ikaros.vesper.player.android.external.internal.relay.VesperRelayDiagnostic
import io.github.ikaros.vesper.player.android.external.internal.relay.VesperRelayFormatAdaptationRequest
import java.io.Closeable
import java.io.File
import java.io.FileOutputStream
import java.io.IOException
import java.io.InterruptedIOException
import java.net.SocketTimeoutException
import java.util.Collections
import java.util.concurrent.Executors
import java.util.concurrent.ThreadFactory
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicReference

internal const val HOST_PREPARED_DASH_INPUT_MODE = "host_prepared_dash_fmp4_tracks"

internal data class VesperRelayPreparedTrack(
    val kind: String,
    val pipePath: String,
    val mediaId: String,
    val mimeType: String?,
    val codecs: String?,
)

internal class VesperRelayHostInputException(
    val status: Int,
    val diagnostic: VesperRelayDiagnostic,
) : Exception(diagnostic.message)

internal class VesperRelayHostInputSession private constructor(
    private val rootDir: File,
    private val plannedTracks: List<VesperRelayDashTrackPlan>,
    private val baseDetails: Map<String, String>,
    private val resolver: VesperRelayDashResourceResolver,
) : Closeable {
    val tracks: List<VesperRelayPreparedTrack> =
        plannedTracks.map { track ->
            VesperRelayPreparedTrack(
                kind = track.kind,
                pipePath = track.pipePath,
                mediaId = track.mediaId,
                mimeType = track.mimeType,
                codecs = track.codecs,
            )
        }

    private val cancelled = AtomicBoolean(false)
    private val failure = AtomicReference<VesperRelayDiagnostic?>()
    private val activeOutputs = Collections.synchronizedSet(mutableSetOf<Closeable>())
    private val executor =
        Executors.newFixedThreadPool(plannedTracks.size.coerceAtLeast(1), VesperHostInputThreadFactory())

    fun start() {
        plannedTracks.forEach { track ->
            executor.execute { writeTrack(track) }
        }
    }

    fun failureDiagnostic(): VesperRelayDiagnostic? = failure.get()

    override fun close() {
        if (!cancelled.compareAndSet(false, true)) {
            return
        }
        resolver.cancel()
        activeOutputs.toList().forEach { output ->
            runCatching { output.close() }
        }
        executor.shutdownNow()
        runCatching { rootDir.deleteRecursively() }
    }

    private fun writeTrack(track: VesperRelayDashTrackPlan) {
        var currentSegmentDetails = track.baseTrackDetails()
        try {
            val fd = Os.open(track.pipePath, OsConstants.O_RDWR, 0)
            val output = FileOutputStream(fd)
            activeOutputs += output
            try {
                output.use { stream ->
                    track.initializationUri?.let { uri ->
                        val initializationRange = track.initializationRange
                        currentSegmentDetails = track.segmentDetails("init", uri, initializationRange)
                        if (initializationRange == null) {
                            resolver.copyTo(
                                uri = uri,
                                output = stream,
                                cancellation = cancelled,
                            )
                        } else {
                            resolver.copyRangeTo(
                                uri = uri,
                                range = initializationRange,
                                output = stream,
                                cancellation = cancelled,
                            )
                        }
                    }
                    track.segments.forEach { segment ->
                        if (cancelled.get()) {
                            throw HostInputCancelledException()
                        }
                        currentSegmentDetails = track.segmentDetails(
                            segment.index.toString(),
                            segment.uri,
                            segment.byteRange,
                        )
                        if (segment.byteRange == null) {
                            resolver.copyTo(
                                uri = segment.uri,
                                output = stream,
                                cancellation = cancelled,
                            )
                        } else {
                            resolver.copyRangeTo(
                                uri = segment.uri,
                                range = segment.byteRange,
                                output = stream,
                                cancellation = cancelled,
                            )
                        }
                    }
                    stream.flush()
                }
            } finally {
                activeOutputs -= output
            }
        } catch (error: HostInputCancelledException) {
            markFailure(
                code = "host_input_cancelled",
                status = 499,
                message = "Host-prepared DASH input was cancelled.",
                details = currentSegmentDetails,
            )
        } catch (error: SocketTimeoutException) {
            markFailure(
                code = "host_fetch_timeout",
                status = 504,
                message = "Timed out while fetching a DASH segment for host-prepared remux input.",
                details = currentSegmentDetails.withHostError(error.message),
            )
        } catch (error: InterruptedIOException) {
            val code = if (cancelled.get()) "host_input_cancelled" else "host_fetch_timeout"
            markFailure(
                code = code,
                status = if (cancelled.get()) 499 else 504,
                message = if (cancelled.get()) {
                    "Host-prepared DASH input was cancelled."
                } else {
                    "Timed out while fetching a DASH segment for host-prepared remux input."
                },
                details = currentSegmentDetails.withHostError(error.message),
            )
        } catch (error: ErrnoException) {
            markFailure(
                code = "ffmpeg_open_failed",
                status = 503,
                message = "Failed to open host-prepared DASH FIFO.",
                details = track.baseTrackDetails() + mapOf("errno" to error.errno.toString()),
            )
        } catch (error: IOException) {
            val code = if (cancelled.get()) "host_input_cancelled" else error.dashResourceErrorCode()
            markFailure(
                code = code,
                status = if (cancelled.get()) 499 else error.dashResourceHttpStatus(),
                message = if (cancelled.get()) {
                    "Host-prepared DASH input was cancelled."
                } else {
                    "Failed to fetch a DASH segment for host-prepared remux input."
                },
                details = currentSegmentDetails.withHostError(error.message),
            )
        } catch (error: Exception) {
            markFailure(
                code = "host_fetch_failed",
                status = 502,
                message = "Failed to prepare DASH input for relay remux.",
                details = currentSegmentDetails.withHostError(error.message),
            )
        }
    }

    private fun markFailure(
        code: String,
        status: Int,
        message: String,
        details: Map<String, String>,
    ) {
        failure.compareAndSet(
            null,
            VesperRelayDiagnostic(
                code = code,
                message = message,
                details = baseDetails + details,
            ),
        )
        close()
    }

    private fun VesperRelayDashTrackPlan.baseTrackDetails(): Map<String, String> =
        mapOf(
            "inputMode" to HOST_PREPARED_DASH_INPUT_MODE,
            "trackKind" to kind,
            "mediaId" to mediaId,
            "pipePath" to pipePath,
        )

    private fun VesperRelayDashTrackPlan.segmentDetails(
        segmentIndex: String,
        uri: String,
        byteRange: VesperRelayDashByteRange?,
    ): Map<String, String> =
        baseTrackDetails() + mapOf(
            "segmentIndex" to segmentIndex,
            "segmentUriHash" to hashForDiagnostic(uri),
        ) + listOfNotNull(
            byteRange?.toHeaderValue()?.let { "byteRange" to it },
        ).toMap()

    private fun Map<String, String>.withHostError(message: String?): Map<String, String> =
        this + listOfNotNull(
            message?.takeIf { it.isNotBlank() }?.let { "hostError" to it },
        ).toMap()

    companion object {
        fun validate(
            context: Context,
            request: VesperRelayFormatAdaptationRequest,
            resolverFactory: VesperRelayDashResourceResolverFactory = VesperRelayDashResourceResolverFactory(),
        ) {
            validateDashSourceProtocol(request)
            val resolver = resolverFactory.create(context, request)
            planHostPreparedDash(request, resolver)
        }

        fun create(
            context: Context,
            request: VesperRelayFormatAdaptationRequest,
            resolverFactory: VesperRelayDashResourceResolverFactory = VesperRelayDashResourceResolverFactory(),
        ): VesperRelayHostInputSession {
            validateDashSourceProtocol(request)
            val resolver = resolverFactory.create(context, request)
            val plan = planHostPreparedDash(request, resolver)
            val rootDir = File(
                context.cacheDir,
                "vesper-relay-ffmpeg-host-input/${safeFileComponent(request.sessionId)}",
            )
            rootDir.deleteRecursively()
            if (!rootDir.mkdirs() && !rootDir.isDirectory) {
                throw request.hostInputException(
                    code = "ffmpeg_open_failed",
                    status = 503,
                    message = "Failed to create host-prepared DASH input cache directory.",
                    details = mapOf("inputMode" to HOST_PREPARED_DASH_INPUT_MODE),
                )
            }
            val plannedTracks = plan.tracks.mapIndexed { index, track ->
                val pipe = File(rootDir, "${index}-${safeFileComponent(track.mediaId)}.fifo")
                runCatching { pipe.delete() }
                try {
                    Os.mkfifo(pipe.absolutePath, OsConstants.S_IRUSR or OsConstants.S_IWUSR)
                } catch (error: ErrnoException) {
                    throw request.hostInputException(
                        code = "ffmpeg_open_failed",
                        status = 503,
                        message = "Failed to create host-prepared DASH FIFO.",
                        details = mapOf(
                            "inputMode" to HOST_PREPARED_DASH_INPUT_MODE,
                            "trackKind" to track.kind,
                            "mediaId" to track.mediaId,
                            "errno" to error.errno.toString(),
                        ),
                    )
                }
                track.copy(pipePath = pipe.absolutePath)
            }
            return VesperRelayHostInputSession(
                rootDir = rootDir,
                plannedTracks = plannedTracks,
                baseDetails = request.hostInputBaseDetails(),
                resolver = resolver,
            )
        }

        private fun validateDashSourceProtocol(request: VesperRelayFormatAdaptationRequest) {
            if (request.source.protocol != VesperPlayerSourceProtocol.Dash) {
                throw request.hostInputException(
                    code = "unsupported_dash_layout",
                    status = 415,
                    message = "Host-prepared relay remux v1 only accepts DASH sources.",
                    details = mapOf("inputMode" to HOST_PREPARED_DASH_INPUT_MODE),
                )
            }
        }

        private fun planHostPreparedDash(
            request: VesperRelayFormatAdaptationRequest,
            resolver: VesperRelayDashResourceResolver,
        ): VesperRelayDashPlan {
            val manifestText =
                try {
                    resolver.readManifest()
                } catch (error: SocketTimeoutException) {
                    throw request.hostInputException(
                        code = "host_fetch_timeout",
                        status = 504,
                        message = "Timed out while fetching DASH MPD for relay remux.",
                        details = resolverDetails(request, resolver).withSegmentHash(request.source.uri),
                    )
                } catch (error: IOException) {
                    throw request.hostInputException(
                        code = error.dashResourceErrorCode(),
                        status = error.dashResourceHttpStatus(),
                        message = "Failed to fetch DASH MPD for relay remux.",
                        details = resolverDetails(request, resolver)
                            .withSegmentHash(request.source.uri)
                            .withHostError(error.message ?: error.javaClass.simpleName),
                    )
                }
            return planHostPreparedDash(
                manifestText = manifestText,
                manifestUri = resolver.manifestLogicalUri,
                sourceOrigin = resolver.origin,
                resolver = resolver,
                baseDetails = request.hostInputBaseDetails(),
            )
        }
    }
}

private fun VesperRelayFormatAdaptationRequest.hostInputException(
    code: String,
    status: Int,
    message: String,
    details: Map<String, String>,
): VesperRelayHostInputException =
    VesperRelayHostInputException(
        status = status,
        diagnostic = VesperRelayDiagnostic(
            code = code,
            message = message,
            details = hostInputBaseDetails() + details,
        ),
    )

private fun safeFileComponent(value: String): String {
    val output = buildString(value.length) {
        value.forEach { character ->
            append(
                if (character.isLetterOrDigit() || character == '.' || character == '_' || character == '-') {
                    character
                } else {
                    '_'
                },
            )
        }
    }
    return output.takeIf { it.isNotBlank() && it != "." && it != ".." } ?: "media"
}

private class VesperHostInputThreadFactory : ThreadFactory {
    override fun newThread(runnable: Runnable): Thread =
        Thread(runnable, "vesper-relay-host-input").apply {
            isDaemon = true
        }
}
