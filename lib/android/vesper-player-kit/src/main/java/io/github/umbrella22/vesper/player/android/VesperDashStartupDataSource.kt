package io.github.umbrella22.vesper.player.android

import android.net.Uri
import androidx.media3.common.C
import androidx.media3.datasource.DataSource
import androidx.media3.datasource.DataSpec
import androidx.media3.datasource.TransferListener

/** The formal Media3 DASH read path consumes exactly the startup cache warmed by the sequence. */
internal class VesperDashStartupDataSource(
    private val upstream: DataSource,
    private val scope: DashStartupScope,
    private val headers: Map<String, String>,
    private val cache: VesperDashStartupCache = VesperDashStartupCache.shared,
) : DataSource {
    private var cached: DashStartupBytes? = null
    private var offset = 0
    override fun addTransferListener(listener: TransferListener) = upstream.addTransferListener(listener)
    override fun open(dataSpec: DataSpec): Long {
        offset = 0
        cached = if (dataSpec.httpMethod == DataSpec.HTTP_METHOD_GET && dataSpec.httpBody == null) {
            val requestHeaders = dashStartupRequestHeaders(headers, dataSpec.httpRequestHeaders)
            cache.read(scope, DashStartupResource(dataSpec.uri.toString(), dataSpec.position,
                dataSpec.length.takeIf { it != C.LENGTH_UNSET.toLong() }), requestHeaders)
        } else null
        return cached?.bytes?.size?.toLong() ?: upstream.open(dataSpec)
    }
    override fun read(buffer: ByteArray, offset: Int, length: Int): Int {
        val value = cached ?: return upstream.read(buffer, offset, length)
        if (length == 0) return 0
        if (this.offset == value.bytes.size) return C.RESULT_END_OF_INPUT
        val count = minOf(length, value.bytes.size - this.offset)
        value.bytes.copyInto(buffer, offset, this.offset, this.offset + count)
        this.offset += count
        return count
    }
    override fun getUri(): Uri? = cached?.finalUri?.let(Uri::parse) ?: upstream.uri
    override fun getResponseHeaders(): Map<String, List<String>> = if (cached != null) emptyMap() else upstream.responseHeaders
    override fun close() {
        if (cached == null) upstream.close()
        cached = null
        offset = 0
    }
}

internal fun dashStartupRequestHeaders(defaults: Map<String, String>, request: Map<String, String>): Map<String, String> =
    defaults.toMutableMap().apply {
        request.forEach { (key, value) ->
            keys.filter { it.equals(key, true) }.forEach(::remove)
            put(key, value)
        }
    }
