package io.github.umbrella22.vesper.player.android.external.internal.relay.ffmpeg

import android.content.Context
import android.os.ParcelFileDescriptor
import android.system.ErrnoException
import android.system.Os
import android.system.OsConstants
import io.github.umbrella22.vesper.player.android.VesperPlayerSourceProtocol
import io.github.umbrella22.vesper.player.android.external.internal.relay.VesperRelayDiagnostic
import io.github.umbrella22.vesper.player.android.external.internal.relay.VesperRelayFormatAdaptationRequest
import io.github.umbrella22.vesper.player.android.external.internal.relay.VesperRelayLifetime
import java.io.Closeable
import java.io.File
import java.io.FileDescriptor
import java.io.IOException
import java.io.InterruptedIOException
import java.io.OutputStream
import java.net.SocketTimeoutException
import java.util.Collections
import java.util.concurrent.Executors
import java.util.concurrent.ThreadFactory
import java.util.concurrent.TimeUnit
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
    private val started = AtomicBoolean(false)
    private val failure = AtomicReference<VesperRelayDiagnostic?>()
    private val activeOutputs = Collections.synchronizedSet(mutableSetOf<Closeable>())
    private val executor =
        Executors.newFixedThreadPool(plannedTracks.size.coerceAtLeast(1), VesperHostInputThreadFactory())

    fun start() {
        if (cancelled.get()) throw HostInputCancelledException()
        if (!started.compareAndSet(false, true)) return
        try {
            plannedTracks.forEach { track ->
                if (cancelled.get()) throw HostInputCancelledException()
                executor.execute { writeTrack(track) }
            }
        } catch (error: java.util.concurrent.RejectedExecutionException) {
            if (cancelled.get()) throw HostInputCancelledException()
            close()
            throw error
        }
    }

    fun failureDiagnostic(): VesperRelayDiagnostic? = failure.get()

    override fun close() {
        if (!cancelled.compareAndSet(false, true)) {
            return
        }
        resolver.cancel()
        val outputs = synchronized(activeOutputs) { activeOutputs.toList() }
        outputs.forEach { output ->
            runCatching { output.close() }
        }
        executor.shutdownNow()
        runCatching { rootDir.deleteRecursively() }
    }

    private fun writeTrack(track: VesperRelayDashTrackPlan) {
        var currentSegmentDetails = track.baseTrackDetails()
        try {
            val output = openHostPreparedDashFifoWriter(track.pipePath, cancelled)
            val registered = synchronized(activeOutputs) {
                if (cancelled.get()) {
                    false
                } else {
                    activeOutputs.add(output)
                    true
                }
            }
            if (!registered) {
                output.close()
                throw HostInputCancelledException()
            }
            try {
                output.use { stream ->
                    if (cancelled.get()) {
                        throw HostInputCancelledException()
                    }
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
            lifetime: VesperRelayLifetime = VesperRelayLifetime(),
        ): VesperRelayHostInputSession {
            validateDashSourceProtocol(request)
            val resolver = resolverFactory.create(context, request)
            val ownedResolver = Closeable { resolver.cancel() }
            if (!lifetime.attach(ownedResolver)) throw HostInputCancelledException()
            var ownedRoot: File? = null
            try {
            lifetime.checkActive()
            val plan = planHostPreparedDash(request, resolver)
            lifetime.checkActive()
            val rootDir = File(
                context.cacheDir,
                "vesper-relay-ffmpeg-host-input/${safeFileComponent(request.sessionId)}-${java.util.UUID.randomUUID()}",
            )
            ownedRoot = rootDir
            lifetime.checkActive()
            if (!rootDir.mkdirs() && !rootDir.isDirectory) {
                throw request.hostInputException(
                    code = "ffmpeg_open_failed",
                    status = 503,
                    message = "Failed to create host-prepared DASH input cache directory.",
                    details = mapOf("inputMode" to HOST_PREPARED_DASH_INPUT_MODE),
                )
            }
            val plannedTracks = plan.tracks.mapIndexed { index, track ->
                lifetime.checkActive()
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
            lifetime.checkActive()
            return VesperRelayHostInputSession(
                rootDir = rootDir,
                plannedTracks = plannedTracks,
                baseDetails = request.hostInputBaseDetails(),
                resolver = resolver,
            )
            } catch (error: Throwable) {
                resolver.cancel()
                ownedRoot?.deleteRecursively()
                throw error
            } finally {
                lifetime.detach(ownedResolver)
            }
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

// Match defaultRelayHttpClient's connection-establishment budget. Waiting for
// the native reader must not create an unbounded FIFO open before media fetch.
private const val HOST_INPUT_FIFO_READER_TIMEOUT_MILLIS = 10_000L
private const val HOST_INPUT_FIFO_READER_POLL_MILLIS = 25L

internal fun openHostPreparedDashFifoWriter(
    pipePath: String,
    cancellation: AtomicBoolean,
    readerTimeoutMillis: Long = HOST_INPUT_FIFO_READER_TIMEOUT_MILLIS,
    createOutputStream: (ParcelFileDescriptor) -> OutputStream = { OwnedDashFifoOutputStream(it) },
): OutputStream {
    val deadline = System.nanoTime() + TimeUnit.MILLISECONDS.toNanos(readerTimeoutMillis)
    val rawFd = awaitHostPreparedDashFifoReader(pipePath, cancellation, deadline)
    var descriptor: ParcelFileDescriptor? = null
    try {
        checkFifoWriterCancellation(cancellation)
        val flags = Os.fcntlInt(rawFd, OsConstants.F_GETFL, 0)
        // Only the open is nonblocking; writes keep pipe backpressure.
        Os.fcntlInt(rawFd, OsConstants.F_SETFL, flags and OsConstants.O_NONBLOCK.inv())
        descriptor = ParcelFileDescriptor.dup(rawFd)
    } finally {
        try {
            Os.close(rawFd)
        } catch (error: Exception) {
            descriptor?.close()
            throw error
        }
    }
    val ownedDescriptor = checkNotNull(descriptor)
    var transferred = false
    try {
        checkFifoWriterCancellation(cancellation)
        val output = createOutputStream(ownedDescriptor)
        transferred = true
        return output
    } finally {
        if (!transferred) {
            ownedDescriptor.close()
        }
    }
}

private fun awaitHostPreparedDashFifoReader(
    pipePath: String,
    cancellation: AtomicBoolean,
    deadline: Long,
): FileDescriptor {
    while (true) {
        checkFifoWriterCancellation(cancellation)
        try {
            return Os.open(pipePath, OsConstants.O_WRONLY or OsConstants.O_NONBLOCK, 0)
        } catch (error: ErrnoException) {
            if (error.errno != OsConstants.ENXIO) {
                throw error
            }
        }
        val remaining = deadline - System.nanoTime()
        if (remaining <= 0) {
            throw SocketTimeoutException("Timed out waiting for the native DASH FIFO reader.")
        }
        try {
            Thread.sleep(
                minOf(
                    HOST_INPUT_FIFO_READER_POLL_MILLIS,
                    TimeUnit.NANOSECONDS.toMillis(remaining).coerceAtLeast(1L),
                ),
            )
        } catch (error: InterruptedException) {
            Thread.currentThread().interrupt()
            throw HostInputCancelledException()
        }
    }
}

private fun checkFifoWriterCancellation(cancellation: AtomicBoolean) {
    if (cancellation.get() || Thread.currentThread().isInterrupted) {
        throw HostInputCancelledException()
    }
}

private class OwnedDashFifoOutputStream(
    descriptor: ParcelFileDescriptor,
) : ParcelFileDescriptor.AutoCloseOutputStream(descriptor) {
    private val closed = AtomicBoolean(false)

    override fun close() {
        if (closed.compareAndSet(false, true)) {
            super.close()
        }
    }
}

internal fun VesperRelayFormatAdaptationRequest.hostInputException(
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
