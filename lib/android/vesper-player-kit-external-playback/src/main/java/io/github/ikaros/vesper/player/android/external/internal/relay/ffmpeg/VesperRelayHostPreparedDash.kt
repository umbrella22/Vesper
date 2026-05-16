package io.github.ikaros.vesper.player.android.external.internal.relay.ffmpeg

import android.content.Context
import android.net.Uri
import android.system.ErrnoException
import android.system.Os
import android.system.OsConstants
import io.github.ikaros.vesper.player.android.VesperPlayerSource
import io.github.ikaros.vesper.player.android.VesperPlayerSourceProtocol
import io.github.ikaros.vesper.player.android.external.internal.relay.VesperRelayDiagnostic
import io.github.ikaros.vesper.player.android.external.internal.relay.VesperRelayFormatAdaptationRequest
import java.io.Closeable
import java.io.File
import java.io.FileNotFoundException
import java.io.FileOutputStream
import java.io.IOException
import java.io.InputStream
import java.io.InterruptedIOException
import java.io.StringReader
import java.net.HttpURLConnection
import java.net.SocketTimeoutException
import java.net.URI
import java.net.URL
import java.security.MessageDigest
import java.util.Collections
import java.util.Locale
import java.util.concurrent.Executors
import java.util.concurrent.ThreadFactory
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicReference
import javax.xml.parsers.DocumentBuilderFactory
import org.w3c.dom.Document
import org.w3c.dom.Element
import org.xml.sax.InputSource

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
                        currentSegmentDetails = track.segmentDetails("init", uri)
                        resolver.copyTo(
                            uri = uri,
                            output = stream,
                            cancellation = cancelled,
                        )
                    }
                    track.segments.forEach { segment ->
                        if (cancelled.get()) {
                            throw HostInputCancelledException()
                        }
                        currentSegmentDetails = track.segmentDetails(segment.index.toString(), segment.uri)
                        resolver.copyTo(
                            uri = segment.uri,
                            output = stream,
                            cancellation = cancelled,
                        )
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
    ): Map<String, String> =
        baseTrackDetails() + mapOf(
            "segmentIndex" to segmentIndex,
            "segmentUriHash" to hashForDiagnostic(uri),
        )

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
                baseDetails = request.hostInputBaseDetails(),
            )
        }
    }
}

internal data class VesperRelayDashSourceOrigin(
    val kind: String,
    val manifestUri: String,
    val rootUri: String,
)

internal open class VesperRelayDashResourceResolver(
    val origin: VesperRelayDashSourceOrigin,
    val manifestLogicalUri: String,
) {
    @Throws(IOException::class)
    open fun readManifest(): String = throw UnsupportedOperationException()

    @Throws(IOException::class, HostInputCancelledException::class)
    open fun copyTo(
        uri: String,
        output: FileOutputStream,
        cancellation: AtomicBoolean,
    ) {
        throw UnsupportedOperationException()
    }

    open fun cancel() = Unit
}

internal class VesperRelayDashResourceResolverFactory {
    fun create(
        context: Context,
        request: VesperRelayFormatAdaptationRequest,
    ): VesperRelayDashResourceResolver {
        val uri = request.source.uri
        return when {
            uri.startsWith("http://", ignoreCase = true) ||
                uri.startsWith("https://", ignoreCase = true) ->
                VesperRelayHttpDashResourceResolver(
                    source = request.source,
                    requestHeaders = request.requestHeaders,
                )
            uri.startsWith("content://", ignoreCase = true) ->
                VesperRelayContentDashResourceResolver(
                    context = context,
                    source = request.source,
                )
            else ->
                fileDashResolver(request.source)
        }
    }
}

private fun fileDashResolver(source: VesperPlayerSource): VesperRelayFileDashResourceResolver =
    VesperRelayFileDashResourceResolver(origin = source.uri.toFileDashOrigin())

internal class VesperRelayHttpDashResourceResolver(
    source: VesperPlayerSource,
    requestHeaders: Map<String, String>,
) : VesperRelayDashResourceResolver(
    origin = VesperRelayDashSourceOrigin(
        kind = "remote",
        manifestUri = source.uri,
        rootUri = source.uri,
    ),
    manifestLogicalUri = source.uri,
) {
    private val headers = mergedRemoteHeaders(source, requestHeaders)
    private val activeConnections = Collections.synchronizedSet(mutableSetOf<HttpURLConnection>())

    override fun readManifest(): String {
        val connection = openConnection(manifestLogicalUri, headers)
        activeConnections += connection
        return try {
            val status = connection.responseCode
            if (status >= 400) {
                throw IOException("HTTP $status")
            }
            connection.inputStream.use { input ->
                input.readBytes().toString(Charsets.UTF_8)
            }
        } finally {
            activeConnections -= connection
            connection.disconnect()
        }
    }

    override fun copyTo(
        uri: String,
        output: FileOutputStream,
        cancellation: AtomicBoolean,
    ) {
        if (cancellation.get()) {
            throw HostInputCancelledException()
        }
        val connection = openConnection(uri, headers)
        activeConnections += connection
        try {
            val status = connection.responseCode
            if (status >= 400) {
                throw IOException("HTTP $status")
            }
            val input = connection.inputStream
            input.use { stream ->
                stream.copyToCancellable(output, cancellation)
            }
        } finally {
            activeConnections -= connection
            connection.disconnect()
        }
    }

    override fun cancel() {
        activeConnections.toList().forEach { connection ->
            runCatching { connection.disconnect() }
        }
    }

    private fun openConnection(
        uri: String,
        headers: Map<String, String>,
    ): HttpURLConnection {
        val connection = URL(uri).openConnection() as HttpURLConnection
        connection.instanceFollowRedirects = true
        connection.connectTimeout = 10_000
        connection.readTimeout = 20_000
        connection.requestMethod = "GET"
        headers.forEach { (name, value) ->
            if (name.isNotBlank() && value.isNotBlank()) {
                connection.setRequestProperty(name, value)
            }
        }
        return connection
    }
}

internal class VesperRelayFileDashResourceResolver internal constructor(
    origin: VesperRelayDashSourceOrigin,
) : VesperRelayDashResourceResolver(
    origin = origin,
    manifestLogicalUri = origin.manifestUri,
) {
    private val rootDirectory = File(URI(origin.rootUri)).canonicalFile

    override fun readManifest(): String {
        val file = fileForLogicalUri(manifestLogicalUri)
        return file.readText(Charsets.UTF_8)
    }

    override fun copyTo(
        uri: String,
        output: FileOutputStream,
        cancellation: AtomicBoolean,
    ) {
        if (cancellation.get()) {
            throw HostInputCancelledException()
        }
        fileForLogicalUri(uri).inputStream().use { input ->
            input.copyToCancellable(output, cancellation)
        }
    }

    private fun fileForLogicalUri(uri: String): File {
        val file =
            try {
                File(URI(uri)).canonicalFile
            } catch (error: Exception) {
                throw DashResourceException(
                    code = "unsupported_dash_source_origin",
                    status = 415,
                    message = "DASH file resource URI is invalid: ${error.message ?: error.javaClass.simpleName}",
                )
            }
        if (!file.toPath().startsWith(rootDirectory.toPath())) {
            throw DashResourceException(
                code = "unsupported_mixed_dash_origin",
                status = 415,
                message = "DASH file resource escapes the manifest directory.",
            )
        }
        if (!file.exists()) {
            throw FileNotFoundException(file.absolutePath)
        }
        return file
    }
}

internal class VesperRelayContentDashResourceResolver(
    context: Context,
    source: VesperPlayerSource,
) : VesperRelayDashResourceResolver(
    origin = source.uri.toContentDashOrigin(),
    manifestLogicalUri = source.uri,
) {
    private val resolver = context.contentResolver
    private val rootUri = Uri.parse(origin.rootUri)

    override fun readManifest(): String {
        return openInput(manifestLogicalUri).use { input ->
            input.readBytes().toString(Charsets.UTF_8)
        }
    }

    override fun copyTo(
        uri: String,
        output: FileOutputStream,
        cancellation: AtomicBoolean,
    ) {
        if (cancellation.get()) {
            throw HostInputCancelledException()
        }
        openInput(uri).use { input ->
            input.copyToCancellable(output, cancellation)
        }
    }

    private fun openInput(uri: String): InputStream {
        val parsed = Uri.parse(uri)
        if (parsed.scheme?.equals("content", ignoreCase = true) != true ||
            parsed.authority != rootUri.authority ||
            !parsed.path.orEmpty().startsWith(rootUri.path.orEmpty())
        ) {
            throw DashResourceException(
                code = "unsupported_mixed_dash_origin",
                status = 415,
                message = "DASH content resource is outside the manifest provider root.",
            )
        }
        return resolver.openInputStream(parsed)
            ?: throw DashResourceException(
                code = "dash_resource_not_found",
                status = 404,
                message = "DASH content resource is not available.",
            )
    }
}

internal class HostInputCancelledException : IOException("Host input cancelled.")

internal data class VesperRelayDashPlan(
    val tracks: List<VesperRelayDashTrackPlan>,
)

internal data class VesperRelayDashTrackPlan(
    val kind: String,
    val mediaId: String,
    val mimeType: String?,
    val codecs: String?,
    val initializationUri: String?,
    val segments: List<VesperRelayDashSegment>,
    val pipePath: String = "",
)

internal data class VesperRelayDashSegment(
    val index: Long,
    val uri: String,
)

private data class DashTemplate(
    val media: String,
    val initialization: String?,
    val startNumber: Long,
    val timescale: Long,
    val duration: Long,
)

internal fun planHostPreparedDash(
    manifestText: String,
    manifestUri: String,
    sourceOrigin: VesperRelayDashSourceOrigin = remoteDashOrigin(manifestUri),
    baseDetails: Map<String, String> = emptyMap(),
): VesperRelayDashPlan {
    val document =
        try {
            parseXmlDocument(manifestText)
        } catch (error: Exception) {
            throw VesperRelayHostInputException(
                status = 415,
                diagnostic = VesperRelayDiagnostic(
                    code = "unsupported_dash_layout",
                    message = "DASH MPD could not be parsed for host-prepared relay remux.",
                    details = baseDetails + mapOf("hostError" to (error.message ?: error.javaClass.simpleName)),
                ),
            )
        }

    val type = document.documentElement.getAttribute("type")
    if (type.isNotBlank() && !type.equals("static", ignoreCase = true)) {
        throw VesperRelayHostInputException(
            status = 415,
            diagnostic = VesperRelayDiagnostic(
                code = "unsupported_dynamic_dash",
                message = "Dynamic DASH MPD is not supported by host-prepared relay remux v1.",
                details = baseDetails + mapOf("inputMode" to HOST_PREPARED_DASH_INPUT_MODE),
            ),
        )
    }
    if (document.getElementsByTagNameNS("*", "ContentProtection").length > 0) {
        throw VesperRelayHostInputException(
            status = 415,
            diagnostic = VesperRelayDiagnostic(
                code = "unsupported_encrypted_dash",
                message = "Encrypted DASH content cannot be remuxed for DLNA fallback.",
                details = baseDetails + mapOf("inputMode" to HOST_PREPARED_DASH_INPUT_MODE),
            ),
        )
    }
    if (document.getElementsByTagNameNS("*", "SegmentTimeline").length > 0) {
        throw VesperRelayHostInputException(
            status = 415,
            diagnostic = VesperRelayDiagnostic(
                code = "unsupported_dash_layout",
                message = "DASH SegmentTimeline is not supported by host-prepared relay remux v1.",
                details = baseDetails + mapOf("inputMode" to HOST_PREPARED_DASH_INPUT_MODE),
            ),
        )
    }

    val durationSeconds = parseIso8601DurationSeconds(
        document.documentElement.getAttribute("mediaPresentationDuration"),
    ) ?: throw VesperRelayHostInputException(
        status = 415,
        diagnostic = VesperRelayDiagnostic(
            code = "unsupported_dash_layout",
            message = "Host-prepared relay remux requires a finite DASH mediaPresentationDuration.",
            details = baseDetails + mapOf("inputMode" to HOST_PREPARED_DASH_INPUT_MODE),
        ),
    )

    val periods = childElementsByTagName(document.documentElement, "Period")
    if (periods.size > 1) {
        throw VesperRelayHostInputException(
            status = 415,
            diagnostic = VesperRelayDiagnostic(
                code = "unsupported_dash_layout",
                message = "Multiple DASH periods are not supported by host-prepared relay remux v1.",
                details = baseDetails + mapOf("inputMode" to HOST_PREPARED_DASH_INPUT_MODE),
            ),
        )
    }

    val mpdBase = firstBaseUrl(document.documentElement)
        ?.let { resolveDashReference(manifestUri, it, sourceOrigin, baseDetails) }
        ?: manifestUri
    val period = periods.firstOrNull()
    val periodBase = period
        ?.let { firstBaseUrl(it) }
        ?.let { resolveDashReference(mpdBase, it, sourceOrigin, baseDetails) }
        ?: mpdBase
    val adaptationSets =
        if (period != null) {
            childElementsByTagName(period, "AdaptationSet")
        } else {
            childElementsByTagName(document.documentElement, "AdaptationSet")
        }

    val planned = mutableListOf<VesperRelayDashTrackPlan>()
    val selectedKinds = mutableSetOf<String>()
    adaptationSets.forEachIndexed { index, adaptation ->
        val representations = childElementsByTagName(adaptation, "Representation")
        val selectedRepresentation = representations.firstOrNull() ?: return@forEachIndexed
        val kind = dashMediaKind(adaptation, selectedRepresentation) ?: return@forEachIndexed
        if (kind in selectedKinds) {
            return@forEachIndexed
        }
        val representationId =
            selectedRepresentation.getAttribute("id").takeIf(String::isNotBlank) ?: "$kind$index"
        val template = dashTemplateFromElement(selectedRepresentation)
            ?: dashTemplateFromElement(adaptation)
            ?: throw unsupportedDashLayout(
                baseDetails = baseDetails,
                message = "Host-prepared relay remux v1 requires SegmentTemplate tracks.",
                details = mapOf("trackKind" to kind, "mediaId" to representationId),
            )
        validateTemplate(template, baseDetails, kind, representationId)
        val segmentSeconds = template.duration.toDouble() / template.timescale.coerceAtLeast(1L).toDouble()
        val segmentCount = kotlin.math.ceil(durationSeconds / segmentSeconds)
            .toLong()
            .coerceAtLeast(1L)
        if (segmentCount > Int.MAX_VALUE) {
            throw unsupportedDashLayout(
                baseDetails = baseDetails,
                message = "DASH SegmentTemplate expands to too many segments for relay remux v1.",
                details = mapOf("trackKind" to kind, "mediaId" to representationId),
            )
        }
        val adaptationBase = firstBaseUrl(adaptation)
            ?.let { resolveDashReference(periodBase, it, sourceOrigin, baseDetails) }
            ?: periodBase
        val representationBase = firstBaseUrl(selectedRepresentation)
            ?.let { resolveDashReference(adaptationBase, it, sourceOrigin, baseDetails) }
            ?: adaptationBase
        val mediaId = "$kind$index"
        val initializationUri = template.initialization?.let { initialization ->
            resolveDashReference(
                representationBase,
                expandDashTemplate(initialization, representationId, template.startNumber),
                sourceOrigin,
                baseDetails,
            )
        }
        val segments = (0 until segmentCount).map { offset ->
            val number = template.startNumber + offset
            VesperRelayDashSegment(
                index = number,
                uri = resolveDashReference(
                    representationBase,
                    expandDashTemplate(template.media, representationId, number),
                    sourceOrigin,
                    baseDetails,
                ),
            )
        }
        planned += VesperRelayDashTrackPlan(
            kind = kind,
            mediaId = mediaId,
            mimeType = selectedRepresentation.getAttribute("mimeType").takeIf(String::isNotBlank)
                ?: adaptation.getAttribute("mimeType").takeIf(String::isNotBlank),
            codecs = selectedRepresentation.getAttribute("codecs").takeIf(String::isNotBlank),
            initializationUri = initializationUri,
            segments = segments,
        )
        selectedKinds += kind
    }

    if (planned.none { it.kind == "video" }) {
        throw unsupportedDashLayout(
            baseDetails = baseDetails,
            message = "DASH MPD did not contain a supported video SegmentTemplate representation.",
            details = mapOf("inputMode" to HOST_PREPARED_DASH_INPUT_MODE),
        )
    }
    return VesperRelayDashPlan(planned.sortedBy { if (it.kind == "video") 0 else 1 })
}

private fun validateTemplate(
    template: DashTemplate,
    baseDetails: Map<String, String>,
    kind: String,
    mediaId: String,
) {
    if (template.duration <= 0L || template.timescale <= 0L) {
        throw unsupportedDashLayout(
            baseDetails = baseDetails,
            message = "DASH SegmentTemplate duration and timescale must be greater than zero.",
            details = mapOf("trackKind" to kind, "mediaId" to mediaId),
        )
    }
    if (template.initialization.isNullOrBlank()) {
        throw unsupportedDashLayout(
            baseDetails = baseDetails,
            message = "Host-prepared fMP4 DASH remux requires an initialization segment.",
            details = mapOf("trackKind" to kind, "mediaId" to mediaId),
        )
    }
    if (!DASH_NUMBER_TOKEN.containsMatchIn(template.media) || template.media.contains("${'$'}Time")) {
        throw unsupportedDashLayout(
            baseDetails = baseDetails,
            message = "Host-prepared relay remux v1 requires SegmentTemplate media with Number tokens.",
            details = mapOf("trackKind" to kind, "mediaId" to mediaId),
        )
    }
}

private fun unsupportedDashLayout(
    baseDetails: Map<String, String>,
    message: String,
    details: Map<String, String>,
): VesperRelayHostInputException =
    VesperRelayHostInputException(
        status = 415,
        diagnostic = VesperRelayDiagnostic(
            code = "unsupported_dash_layout",
            message = message,
            details = baseDetails + mapOf("inputMode" to HOST_PREPARED_DASH_INPUT_MODE) + details,
        ),
    )

private fun unsupportedMixedDashOrigin(
    baseDetails: Map<String, String>,
    origin: VesperRelayDashSourceOrigin,
    resolvedUri: String,
): VesperRelayHostInputException =
    VesperRelayHostInputException(
        status = 415,
        diagnostic = VesperRelayDiagnostic(
            code = "unsupported_mixed_dash_origin",
            message = "DASH references must stay within the source origin for relay remux.",
            details = baseDetails + mapOf(
                "inputMode" to HOST_PREPARED_DASH_INPUT_MODE,
                "sourceOrigin" to origin.kind,
                "resourceUriHash" to hashForDiagnostic(resolvedUri),
            ),
        ),
    )

private fun parseXmlDocument(xmlText: String): Document {
    val factory = DocumentBuilderFactory.newInstance().apply {
        isNamespaceAware = true
        runCatching { setFeature("http://apache.org/xml/features/disallow-doctype-decl", true) }
        runCatching { setFeature("http://xml.org/sax/features/external-general-entities", false) }
        runCatching { setFeature("http://xml.org/sax/features/external-parameter-entities", false) }
    }
    return factory
        .newDocumentBuilder()
        .parse(InputSource(StringReader(xmlText)))
}

private fun dashTemplateFromElement(element: Element): DashTemplate? {
    val template = childElementsByTagName(element, "SegmentTemplate").firstOrNull() ?: return null
    return DashTemplate(
        media = template.getAttribute("media").takeIf(String::isNotBlank) ?: return null,
        initialization = template.getAttribute("initialization").takeIf(String::isNotBlank),
        startNumber = template.getAttribute("startNumber").toLongOrNull() ?: 1L,
        timescale = template.getAttribute("timescale").toLongOrNull() ?: 1L,
        duration = template.getAttribute("duration").toLongOrNull() ?: 0L,
    )
}

private fun dashMediaKind(
    adaptation: Element,
    representation: Element,
): String? {
    val mimeType = sequenceOf(
        representation.getAttribute("mimeType"),
        adaptation.getAttribute("mimeType"),
        adaptation.getAttribute("contentType"),
    ).firstOrNull { it.isNotBlank() }
    when {
        mimeType?.startsWith("video/", ignoreCase = true) == true -> return "video"
        mimeType?.startsWith("audio/", ignoreCase = true) == true -> return "audio"
        mimeType.equals("video", ignoreCase = true) -> return "video"
        mimeType.equals("audio", ignoreCase = true) -> return "audio"
    }
    val codecs = representation.getAttribute("codecs").lowercase(Locale.US)
    return when {
        codecs.startsWith("mp4a") || codecs.startsWith("ac-3") || codecs.startsWith("ec-3") -> "audio"
        codecs.startsWith("avc") ||
            codecs.startsWith("hvc") ||
            codecs.startsWith("hev") ||
            codecs.startsWith("av01") -> "video"
        else -> null
    }
}

private fun childElementsByTagName(
    parent: Element,
    tagName: String,
): List<Element> =
    buildList {
        val children = parent.childNodes
        for (index in 0 until children.length) {
            val child = children.item(index) as? Element ?: continue
            if (child.localName == tagName || child.tagName == tagName) {
                add(child)
            }
        }
    }

private fun firstBaseUrl(element: Element): String? =
    childElementsByTagName(element, "BaseURL")
        .firstOrNull()
        ?.textContent
        ?.trim()
        ?.takeIf { it.isNotEmpty() }

private fun parseIso8601DurationSeconds(value: String?): Double? {
    if (value.isNullOrBlank() || !value.startsWith("PT")) {
        return null
    }
    var number = ""
    var total = 0.0
    value.drop(2).forEach { character ->
        if (character.isDigit() || character == '.') {
            number += character
            return@forEach
        }
        val parsed = number.toDoubleOrNull() ?: return null
        number = ""
        when (character) {
            'H' -> total += parsed * 3600.0
            'M' -> total += parsed * 60.0
            'S' -> total += parsed
            else -> return null
        }
    }
    return total.takeIf { it > 0.0 }
}

private val DASH_NUMBER_TOKEN = Regex("""\${'$'}Number(?:%0(\d+)d)?\${'$'}""")

private fun expandDashTemplate(
    template: String,
    representationId: String,
    number: Long,
): String =
    DASH_NUMBER_TOKEN
        .replace(template.replace("${'$'}RepresentationID${'$'}", representationId)) { match ->
            val width = match.groupValues.getOrNull(1)?.toIntOrNull()
            if (width == null) {
                number.toString()
            } else {
                number.toString().padStart(width, '0')
            }
        }

private fun resolveDashReference(
    baseUri: String,
    reference: String,
    origin: VesperRelayDashSourceOrigin,
    baseDetails: Map<String, String>,
): String {
    val resolved =
        runCatching {
            val ref = URI(reference)
            if (ref.isAbsolute || baseUri.isBlank()) {
                ref.toString()
            } else {
                URI(baseUri).resolve(ref).normalize().toString()
            }
        }.getOrElse { reference }
    val scheme = resolved.substringBefore(':', missingDelimiterValue = "").lowercase(Locale.US)
    return when (origin.kind) {
        "remote" -> {
            if (scheme == "http" || scheme == "https") {
                resolved
            } else {
                throw unsupportedMixedDashOrigin(baseDetails, origin, resolved)
            }
        }
        "file" -> {
            if (scheme == "file") {
                val root = File(URI(origin.rootUri)).canonicalFile.toPath()
                val candidate = File(URI(resolved)).canonicalFile.toPath()
                if (candidate.startsWith(root)) {
                    resolved
                } else {
                    throw unsupportedMixedDashOrigin(baseDetails, origin, resolved)
                }
            } else {
                throw unsupportedMixedDashOrigin(baseDetails, origin, resolved)
            }
        }
        "content" -> {
            if (scheme == "content" && contentUriWithinRoot(resolved, origin.rootUri)) {
                resolved
            } else {
                throw unsupportedMixedDashOrigin(baseDetails, origin, resolved)
            }
        }
        else -> throw unsupportedMixedDashOrigin(baseDetails, origin, resolved)
    }
}

private fun mergedRemoteHeaders(
    source: VesperPlayerSource,
    requestHeaders: Map<String, String>,
): Map<String, String> {
    val merged = linkedMapOf<String, String>()
    source.headers.forEach { (name, value) ->
        if (name.isRemoteFetchHeaderAllowed() && value.isNotBlank()) {
            merged[name] = value
        }
    }
    requestHeaders.forEach { (name, value) ->
        if (name.isRemoteFetchHeaderAllowed() && value.isNotBlank()) {
            merged[name] = value
        }
    }
    return merged
}

private fun String.isRemoteFetchHeaderAllowed(): Boolean =
    lowercase(Locale.US) !in setOf(
        "connection",
        "keep-alive",
        "proxy-authenticate",
        "proxy-authorization",
        "te",
        "trailer",
        "transfer-encoding",
        "upgrade",
        "host",
        "range",
    )

private class DashResourceException(
    val code: String,
    val status: Int,
    override val message: String,
) : IOException(message)

private fun IOException.dashResourceErrorCode(): String =
    when (this) {
        is DashResourceException -> code
        is FileNotFoundException -> "dash_resource_not_found"
        else -> message?.httpErrorCode() ?: "host_fetch_failed"
    }

private fun IOException.dashResourceHttpStatus(): Int =
    when (this) {
        is DashResourceException -> status
        is FileNotFoundException -> 404
        else -> message?.httpErrorStatus() ?: 502
    }

private fun String.httpErrorStatus(): Int? =
    Regex("""HTTP\s+(\d{3})""", RegexOption.IGNORE_CASE)
        .find(this)
        ?.groupValues
        ?.getOrNull(1)
        ?.toIntOrNull()

private fun String.httpErrorCode(): String? =
    httpErrorStatus()?.let { status ->
        if (status == 401 || status == 403) {
            "dash_resource_permission_denied"
        } else if (status == 404) {
            "dash_resource_not_found"
        } else {
            "host_fetch_failed"
        }
    }

private fun resolverDetails(
    request: VesperRelayFormatAdaptationRequest,
    resolver: VesperRelayDashResourceResolver,
): Map<String, String> =
    request.hostInputBaseDetails() + mapOf(
        "sourceOrigin" to resolver.origin.kind,
        "sourceKind" to request.source.kind.name,
        "sourceProtocol" to request.source.protocol.name,
        "uriScheme" to request.source.uri.substringBefore(':', missingDelimiterValue = "").lowercase(Locale.US),
    )

private fun Map<String, String>.withSegmentHash(uri: String): Map<String, String> =
    this + mapOf("segmentUriHash" to hashForDiagnostic(uri))

private fun Map<String, String>.withHostError(message: String?): Map<String, String> =
    this + listOfNotNull(message?.takeIf { it.isNotBlank() }?.let { "hostError" to it }).toMap()

private fun remoteDashOrigin(uri: String): VesperRelayDashSourceOrigin =
    VesperRelayDashSourceOrigin(
        kind = "remote",
        manifestUri = uri,
        rootUri = uri,
    )

private fun String.toFileDashOrigin(): VesperRelayDashSourceOrigin {
    val manifestFile = toLocalDashFile().canonicalFile
    val root = manifestFile.parentFile?.canonicalFile ?: manifestFile.canonicalFile
    return VesperRelayDashSourceOrigin(
        kind = "file",
        manifestUri = manifestFile.toURI().toString(),
        rootUri = root.toURI().toString(),
    )
}

private fun String.toLocalDashFile(): File =
    if (startsWith("file://", ignoreCase = true)) {
        File(URI(this))
    } else {
        File(this)
    }

private fun String.toContentDashOrigin(): VesperRelayDashSourceOrigin {
    val uri = URI(this)
    val path = uri.path.orEmpty()
    val rootPath = path.substringBeforeLast('/', missingDelimiterValue = "")
    val rootUri = URI(uri.scheme, uri.authority, rootPath, null, null).toString()
    return VesperRelayDashSourceOrigin(
        kind = "content",
        manifestUri = this,
        rootUri = rootUri,
    )
}

private fun contentUriWithinRoot(uri: String, rootUri: String): Boolean {
    val parsed = runCatching { URI(uri) }.getOrNull() ?: return false
    val root = runCatching { URI(rootUri) }.getOrNull() ?: return false
    return parsed.scheme?.equals("content", ignoreCase = true) == true &&
        parsed.authority == root.authority &&
        parsed.path.orEmpty().startsWith(root.path.orEmpty())
}

internal fun VesperRelayFormatAdaptationRequest.hostInputBaseDetails(): Map<String, String> =
    mapOf(
        "sessionId" to sessionId,
        "fallbackFormat" to fallbackFormat.name,
        "resourcePath" to resourcePath,
        "inputMode" to HOST_PREPARED_DASH_INPUT_MODE,
        "sourceUriHash" to hashForDiagnostic(source.uri),
        "sourceKind" to source.kind.name,
        "sourceProtocol" to source.protocol.name,
        "uriScheme" to source.uri.substringBefore(':', missingDelimiterValue = "").lowercase(Locale.US),
    ) + listOfNotNull(
        routeId?.let { "routeId" to it },
        routeName?.let { "routeName" to it },
    ).toMap()

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

internal fun hashForDiagnostic(value: String): String {
    val digest = MessageDigest.getInstance("SHA-256").digest(value.toByteArray(Charsets.UTF_8))
    return digest.take(8).joinToString("") { byte -> "%02x".format(byte) }
}

private fun InputStream.copyToCancellable(
    output: FileOutputStream,
    cancellation: AtomicBoolean,
) {
    val buffer = ByteArray(DEFAULT_BUFFER_SIZE)
    while (true) {
        if (cancellation.get()) {
            throw HostInputCancelledException()
        }
        val read = read(buffer)
        if (read < 0) {
            return
        }
        output.write(buffer, 0, read)
    }
}

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
