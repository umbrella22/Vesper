package io.github.ikaros.vesper.player.android.external.internal.relay.ffmpeg

import android.content.Context
import android.net.Uri
import io.github.ikaros.vesper.player.android.VesperPlayerSource
import io.github.ikaros.vesper.player.android.external.internal.relay.VesperRelayDashRemoteMediaPolicy
import java.io.File
import java.io.FileNotFoundException
import java.io.InputStream
import java.io.OutputStream
import java.io.RandomAccessFile
import java.net.URI
import java.util.concurrent.atomic.AtomicBoolean

internal class VesperRelayHttpDashResourceResolver(
    source: VesperPlayerSource,
    requestHeaders: Map<String, String>,
    remoteMediaPolicy: VesperRelayDashRemoteMediaPolicy = VesperRelayDashRemoteMediaPolicy(),
) : VesperRelayDashResourceResolver(
    origin = VesperRelayDashSourceOrigin(
        kind = "remote",
        manifestUri = source.uri,
        rootUri = source.uri,
    ),
    manifestLogicalUri = source.uri,
) {
    private val remoteClient = VesperRelayRemoteDashResourceClient(
        headers = mergedRemoteHeaders(source, requestHeaders),
        allowPrivateAddresses = remoteMediaPolicy.allowPrivateAddresses,
    )

    override fun readManifest(): String =
        remoteClient.readUtf8(manifestLogicalUri)

    override fun copyTo(
        uri: String,
        output: OutputStream,
        cancellation: AtomicBoolean,
    ) = remoteClient.copyTo(uri, output, cancellation)

    override fun copyRangeTo(
        uri: String,
        range: VesperRelayDashByteRange,
        output: OutputStream,
        cancellation: AtomicBoolean,
    ) = remoteClient.copyRangeTo(uri, range, output, cancellation)

    override fun cancel() = remoteClient.cancel()
}

internal class VesperRelayFileDashResourceResolver internal constructor(
    origin: VesperRelayDashSourceOrigin,
    remoteHeaders: Map<String, String> = emptyMap(),
    remoteMediaPolicy: VesperRelayDashRemoteMediaPolicy = VesperRelayDashRemoteMediaPolicy(),
) : VesperRelayDashResourceResolver(
    origin = origin,
    manifestLogicalUri = origin.manifestUri,
) {
    private val rootDirectory = File(URI(origin.rootUri)).canonicalFile
    private val remoteClient = origin.remoteMediaClient(
        headers = remoteHeaders.filterRemoteFetchHeaders(remoteMediaPolicy.allowedRequestHeaders),
        remoteMediaPolicy = remoteMediaPolicy,
    )

    override fun readManifest(): String {
        val file = fileForLogicalUri(manifestLogicalUri)
        return file.readText(Charsets.UTF_8)
    }

    override fun copyTo(
        uri: String,
        output: OutputStream,
        cancellation: AtomicBoolean,
    ) {
        if (cancellation.get()) {
            throw HostInputCancelledException()
        }
        if (uri.isRemoteDashUri()) {
            remoteClientFor(uri).copyTo(uri, output, cancellation)
            return
        }
        fileForLogicalUri(uri).inputStream().use { input ->
            input.copyToCancellable(output, cancellation)
        }
    }

    override fun copyRangeTo(
        uri: String,
        range: VesperRelayDashByteRange,
        output: OutputStream,
        cancellation: AtomicBoolean,
    ) {
        if (cancellation.get()) {
            throw HostInputCancelledException()
        }
        if (uri.isRemoteDashUri()) {
            remoteClientFor(uri).copyRangeTo(uri, range, output, cancellation)
            return
        }
        val file = fileForLogicalUri(uri)
        RandomAccessFile(file, "r").use { input ->
            if (range.end >= input.length()) {
                throw DashResourceException(
                    code = "dash_resource_not_found",
                    status = 416,
                    message = "DASH file resource is shorter than requested byte range.",
                )
            }
            input.seek(range.start)
            input.copyLimitedToCancellable(output, range.length, cancellation)
        }
    }

    override fun cancel() {
        remoteClient?.cancel()
    }

    private fun remoteClientFor(uri: String): VesperRelayRemoteDashResourceClient {
        return remoteClient ?: throw DashResourceException(
            code = "unsupported_mixed_dash_origin",
            status = 415,
            message = "DASH file resolver cannot fetch remote media references.",
        )
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
    remoteHeaders: Map<String, String> = emptyMap(),
    remoteMediaPolicy: VesperRelayDashRemoteMediaPolicy = VesperRelayDashRemoteMediaPolicy(),
) : VesperRelayDashResourceResolver(
    origin = source.uri.toContentDashOrigin(
        allowRemoteMediaReferences = remoteMediaPolicy.allowRemoteReferences,
    ),
    manifestLogicalUri = source.uri,
) {
    private val resolver = context.contentResolver
    private val rootUri = Uri.parse(origin.rootUri)
    private val remoteClient = origin.remoteMediaClient(
        headers = remoteHeaders.filterRemoteFetchHeaders(remoteMediaPolicy.allowedRequestHeaders),
        remoteMediaPolicy = remoteMediaPolicy,
    )

    override fun readManifest(): String {
        return openInput(manifestLogicalUri).use { input ->
            input.readBytes().toString(Charsets.UTF_8)
        }
    }

    override fun copyTo(
        uri: String,
        output: OutputStream,
        cancellation: AtomicBoolean,
    ) {
        if (cancellation.get()) {
            throw HostInputCancelledException()
        }
        if (uri.isRemoteDashUri()) {
            remoteClientFor(uri).copyTo(uri, output, cancellation)
            return
        }
        openInput(uri).use { input ->
            input.copyToCancellable(output, cancellation)
        }
    }

    override fun copyRangeTo(
        uri: String,
        range: VesperRelayDashByteRange,
        output: OutputStream,
        cancellation: AtomicBoolean,
    ) {
        if (cancellation.get()) {
            throw HostInputCancelledException()
        }
        if (uri.isRemoteDashUri()) {
            remoteClientFor(uri).copyRangeTo(uri, range, output, cancellation)
            return
        }
        openInput(uri).use { input ->
            input.skipFullyCancellable(range.start, cancellation)
            input.copyLimitedToCancellable(output, range.length, cancellation)
        }
    }

    override fun cancel() {
        remoteClient?.cancel()
    }

    private fun remoteClientFor(uri: String): VesperRelayRemoteDashResourceClient {
        return remoteClient ?: throw DashResourceException(
            code = "unsupported_mixed_dash_origin",
            status = 415,
            message = "DASH content resolver cannot fetch remote media references.",
        )
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

private fun VesperRelayDashSourceOrigin.remoteMediaClient(
    headers: Map<String, String>,
    remoteMediaPolicy: VesperRelayDashRemoteMediaPolicy,
): VesperRelayRemoteDashResourceClient? =
    if (allowRemoteMediaReferences) {
        VesperRelayRemoteDashResourceClient(
            headers = headers,
            allowPrivateAddresses = remoteMediaPolicy.allowPrivateAddresses,
        )
    } else {
        null
    }
