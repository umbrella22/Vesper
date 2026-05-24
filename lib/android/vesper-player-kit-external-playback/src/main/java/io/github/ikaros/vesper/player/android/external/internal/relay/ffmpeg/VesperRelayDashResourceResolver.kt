package io.github.ikaros.vesper.player.android.external.internal.relay.ffmpeg

import android.content.Context
import io.github.ikaros.vesper.player.android.VesperPlayerSource
import io.github.ikaros.vesper.player.android.external.internal.relay.VesperRelayDashRemoteMediaPolicy
import io.github.ikaros.vesper.player.android.external.internal.relay.VesperRelayFormatAdaptationRequest
import java.io.ByteArrayOutputStream
import java.io.IOException
import java.io.OutputStream
import java.util.concurrent.atomic.AtomicBoolean

internal data class VesperRelayDashSourceOrigin(
    val kind: String,
    val manifestUri: String,
    val rootUri: String,
    val allowRemoteMediaReferences: Boolean = false,
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
        output: OutputStream,
        cancellation: AtomicBoolean,
    ) {
        throw UnsupportedOperationException()
    }

    @Throws(IOException::class, HostInputCancelledException::class)
    open fun copyRangeTo(
        uri: String,
        range: VesperRelayDashByteRange,
        output: OutputStream,
        cancellation: AtomicBoolean,
    ) {
        throw UnsupportedOperationException()
    }

    @Throws(IOException::class)
    open fun readRange(
        uri: String,
        range: VesperRelayDashByteRange,
    ): ByteArray {
        ByteArrayOutputStream(range.lengthAsInt()).use { output ->
            copyRangeTo(uri, range, output, AtomicBoolean(false))
            return output.toByteArray()
        }
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
                    remoteMediaPolicy = request.dashRemoteMediaPolicy,
                )
            uri.startsWith("content://", ignoreCase = true) ->
                VesperRelayContentDashResourceResolver(
                    context = context,
                    source = request.source,
                    remoteHeaders = mergedRemoteHeaders(
                        source = request.source,
                        requestHeaders = request.requestHeaders,
                        allowedHeaderNames = request.dashRemoteMediaPolicy.allowedRequestHeaders,
                    ),
                    remoteMediaPolicy = request.dashRemoteMediaPolicy,
                )
            else ->
                fileDashResolver(request.source, request.requestHeaders, request.dashRemoteMediaPolicy)
        }
    }
}

private fun fileDashResolver(
    source: VesperPlayerSource,
    requestHeaders: Map<String, String>,
    remoteMediaPolicy: VesperRelayDashRemoteMediaPolicy,
): VesperRelayFileDashResourceResolver =
    VesperRelayFileDashResourceResolver(
        origin = source.uri.toFileDashOrigin(
            allowRemoteMediaReferences = remoteMediaPolicy.allowRemoteReferences,
        ),
        remoteHeaders = mergedRemoteHeaders(
            source = source,
            requestHeaders = requestHeaders,
            allowedHeaderNames = remoteMediaPolicy.allowedRequestHeaders,
        ),
        remoteMediaPolicy = remoteMediaPolicy,
	    )
