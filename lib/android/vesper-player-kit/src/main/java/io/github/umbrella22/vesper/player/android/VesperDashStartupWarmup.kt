package io.github.umbrella22.vesper.player.android

import java.io.ByteArrayOutputStream
import java.io.StringReader
import java.net.HttpURLConnection
import java.net.URI
import java.nio.ByteBuffer
import java.nio.ByteOrder
import javax.xml.parsers.DocumentBuilderFactory
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.ensureActive
import org.w3c.dom.Element
import org.xml.sax.InputSource

internal data class DashStartupRepresentation(
    val initialization: DashStartupResource, val index: DashStartupResource,
    val bandwidth: Long, val kind: String, val codec: String,
)

internal object VesperDashStartupPlanner {
    fun parseManifest(data: ByteArray, finalUri: String): List<DashStartupRepresentation> {
        require(data.size <= 1024 * 1024) { "DASH manifest exceeds startup budget" }
        val xml = Charsets.UTF_8.newDecoder().decode(ByteBuffer.wrap(data)).toString()
        require(!xml.contains("<!DOCTYPE", true) && !xml.contains("<!ENTITY", true)) { "DASH external entities are unsupported" }
        val builder = DocumentBuilderFactory.newInstance().apply {
            isNamespaceAware = true
            // Android's DOM factory does not implement the desktop SAX feature switches.
            // DTD/entity declarations are rejected above, before any parser I/O.
        }.newDocumentBuilder()
        builder.setEntityResolver { _, _ -> throw IllegalArgumentException("DASH external entities are unsupported") }
        val root = builder.parse(InputSource(StringReader(xml))).documentElement
        require(root.localName == "MPD" && root.getAttribute("type") != "dynamic") { "Only static DASH can be warmed" }
        require(root.getElementsByTagNameNS("*", "ContentProtection").length == 0) { "Encrypted DASH warmup is unsupported" }
        val periods = children(root, "Period")
        require(periods.size == 1) { "DASH startup requires one period" }
        val period = periods.single()
        val candidates = mutableListOf<DashStartupRepresentation>()
        for (adaptation in children(period, "AdaptationSet")) {
            for (representation in children(adaptation, "Representation")) {
                require(candidates.size < 128) { "DASH representation budget exceeded" }
                val ancestors = listOf(root, period, adaptation, representation)
                if (ancestors.any { children(it, "SegmentTemplate").isNotEmpty() || children(it, "SegmentList").isNotEmpty() }) continue
                val baseElements = ancestors.flatMap { children(it, "SegmentBase") }
                if (baseElements.isEmpty()) continue
                val indexText = baseElements.asReversed().map { it.getAttribute("indexRange") }.firstOrNull { it.isNotEmpty() } ?: continue
                val init = baseElements.asReversed().firstNotNullOfOrNull { children(it, "Initialization").firstOrNull() } ?: continue
                var uri = URI(finalUri)
                ancestors.forEach { element -> children(element, "BaseURL").firstOrNull()?.textContent?.trim()?.let { uri = uri.resolve(it) } }
                val mime = representation.getAttribute("mimeType").ifEmpty { adaptation.getAttribute("mimeType") }
                val kind = adaptation.getAttribute("contentType").ifEmpty { mime.substringBefore('/') }
                if (kind !in setOf("audio", "video")) continue
                val codec = representation.getAttribute("codecs").ifEmpty { adaptation.getAttribute("codecs") }
                val initUri = init.getAttribute("sourceURL").takeIf { it.isNotEmpty() }?.let { uri.resolve(it) } ?: uri
                candidates += DashStartupRepresentation(range(initUri.toString(), init.getAttribute("range")),
                    range(uri.toString(), indexText), representation.getAttribute("bandwidth").toLongOrNull() ?: Long.MAX_VALUE,
                    kind, codec)
            }
        }
        // Warm one conservative startup candidate per media kind. ABR can still
        // choose a different representation; a cache hit is never promised.
        val selected = listOf("video", "audio").mapNotNull { kind ->
            candidates.filter { it.kind == kind }.minWithOrNull(compareBy<DashStartupRepresentation>(
                { if (it.codec.startsWith(if (kind == "video") "avc" else "mp4a.40.")) 0 else 1 }, { it.bandwidth }))
        }
        require(selected.isNotEmpty()) { "No SegmentBase startup candidates" }
        return selected
    }

    fun firstMedia(index: DashStartupResource, data: ByteArray): DashStartupResource {
        require(data.size >= 32 && data.size <= 1024 * 1024)
        val buffer = ByteBuffer.wrap(data).order(ByteOrder.BIG_ENDIAN)
        val size = buffer.int.toLong() and 0xffffffffL
        require(buffer.int == 0x73696478 && size == data.size.toLong()) { "Startup index must contain one complete SIDX box" }
        val version = buffer.get().toInt() and 0xff
        buffer.position(buffer.position() + 3)
        buffer.int // reference_ID
        require(buffer.int != 0) { "SIDX timescale must be positive" }
        val firstOffset = when (version) {
            0 -> { buffer.int; buffer.int.toLong() and 0xffffffffL }
            1 -> { buffer.long; buffer.long.also { require(it >= 0) } }
            else -> error("Unsupported SIDX version")
        }
        buffer.short
        val count = buffer.short.toInt() and 0xffff
        require(count > 0 && buffer.remaining() == count * 12) { "Invalid SIDX references" }
        val reference = buffer.int.toLong() and 0xffffffffL
        require(reference and 0x80000000L == 0L && reference > 0) { "Hierarchical/empty SIDX is unsupported" }
        require(buffer.int != 0) { "SIDX duration must be positive" }
        val start = Math.addExact(Math.addExact(index.position, requireNotNull(index.length)), firstOffset)
        return DashStartupResource(index.uri, start, reference)
    }

    private fun range(uri: String, value: String): DashStartupResource {
        val match = Regex("([0-9]+)-([0-9]+)").matchEntire(value) ?: error("Missing DASH byte range")
        val start = match.groupValues[1].toLong()
        val end = match.groupValues[2].toLong()
        require(end >= start)
        return DashStartupResource(uri, start, Math.addExact(end - start, 1))
    }
    private fun children(element: Element, name: String): List<Element> = (0 until element.childNodes.length)
        .mapNotNull { element.childNodes.item(it) as? Element }.filter { it.localName == name }
}

internal fun interface DashStartupTransport {
    suspend fun fetch(resource: DashStartupResource, headers: Map<String, String>, maximumBytes: Long, timeoutMs: Long): DashStartupBytes
}

internal object DashStartupHttpTransport : DashStartupTransport {
    override suspend fun fetch(resource: DashStartupResource, headers: Map<String, String>, maximumBytes: Long, timeoutMs: Long): DashStartupBytes {
        require(maximumBytes in 1..VesperDashStartupCache.MAX_RESOURCE_BYTES)
        var uri = URI(resource.uri)
        repeat(5) { redirect ->
            currentCoroutineContext().ensureActive()
            require(uri.scheme in setOf("http", "https") && uri.userInfo == null) { "Unsupported DASH resource URL" }
            val connection = uri.toURL().openConnection() as HttpURLConnection
            try {
                connection.instanceFollowRedirects = false
                connection.connectTimeout = timeoutMs.coerceIn(1, 5_000).toInt()
                connection.readTimeout = timeoutMs.coerceIn(1, 5_000).toInt()
                headers.forEach(connection::setRequestProperty)
                connection.setRequestProperty("Accept-Encoding", "identity")
                resource.length?.let { connection.setRequestProperty("Range", "bytes=${resource.position}-${resource.position + it - 1}") }
                val status = connection.responseCode
                if (status in listOf(301, 302, 303, 307, 308)) {
                    require(redirect < 4) { "Too many DASH redirects" }
                    val next = uri.resolve(requireNotNull(connection.getHeaderField("Location")))
                    require(headers.isEmpty() || (next.scheme == uri.scheme && next.host == uri.host && next.port == uri.port)) {
                        "Cross-origin authenticated DASH redirect is unsupported"
                    }
                    uri = next
                    return@repeat
                }
                if (status !in 200..299) throw VesperSequenceWarmupHttpStatusException(status)
                require(connection.getHeaderField("Content-Encoding").let { it == null || it.equals("identity", true) }) { "Encoded DASH range is unsupported" }
                if (resource.length != null) {
                    require(status == 206) { "DASH server ignored the requested range" }
                    val match = Regex("bytes ([0-9]+)-([0-9]+)/([0-9]+|\\*)").matchEntire(connection.getHeaderField("Content-Range") ?: "")
                        ?: error("Invalid DASH Content-Range")
                    val start = match.groupValues[1].toLong()
                    val end = match.groupValues[2].toLong()
                    require(start == resource.position && end == resource.position + resource.length - 1)
                    if (match.groupValues[3] != "*") require(match.groupValues[3].toLong() > end)
                } else require(status == 200)
                require(connection.contentLengthLong <= maximumBytes) { "DASH resource exceeds startup budget" }
                val output = ByteArrayOutputStream()
                connection.inputStream.use { input ->
                    val buffer = ByteArray(16 * 1024)
                    while (true) {
                        currentCoroutineContext().ensureActive()
                        val count = input.read(buffer, 0, minOf(buffer.size.toLong(), maximumBytes - output.size() + 1).toInt())
                        if (count == -1) break
                        if (count == 0) continue
                        require(output.size().toLong() + count <= maximumBytes) { "DASH resource exceeds startup budget" }
                        output.write(buffer, 0, count)
                    }
                }
                val data = output.toByteArray()
                require(connection.contentLengthLong < 0 || connection.contentLengthLong == data.size.toLong()) { "Truncated DASH response" }
                require(data.isNotEmpty() && (resource.length == null || data.size.toLong() == resource.length)) { "Truncated DASH range" }
                currentCoroutineContext().ensureActive()
                return DashStartupBytes(resource, data, uri.toString())
            } finally { connection.disconnect() }
        }
        error("DASH redirect limit exceeded")
    }
}

internal suspend fun warmDashStartup(
    source: VesperPlayerSource, scope: DashStartupScope, timeoutMs: Long,
    transport: DashStartupTransport = DashStartupHttpTransport,
    cache: VesperDashStartupCache = VesperDashStartupCache.shared,
    commitFence: (() -> Boolean) -> Boolean = { it() },
    maximumBytes: Long = VesperDashStartupCache.MAX_WARMUP_BYTES,
): Pair<Long, Boolean> {
    require(source.drmConfiguration == null && source.headers.keys.none { it.equals("Range", true) })
    val generation = cache.currentGeneration()
    val staged = mutableListOf<DashStartupBytes>()
    var total = 0L
    var allHit = true
    suspend fun load(resource: DashStartupResource, limit: Long): DashStartupBytes {
        currentCoroutineContext().ensureActive()
        val maximum = minOf(limit, minOf(maximumBytes, VesperDashStartupCache.MAX_WARMUP_BYTES) - total)
        require(maximum > 0 && (resource.length ?: 0) <= maximum) { "DASH startup budget exceeded" }
        val cached = cache.read(scope, resource, source.headers)
        val value = cached ?: transport.fetch(resource, source.headers, maximum, timeoutMs).also { allHit = false }
        require(value.bytes.size <= maximum && (resource.length == null || value.bytes.size.toLong() == resource.length))
        total += value.bytes.size
        staged += value
        return value
    }
    val manifest = load(DashStartupResource(source.uri), 1024 * 1024)
    val selected = VesperDashStartupPlanner.parseManifest(manifest.bytes, manifest.finalUri)
    for (representation in selected) {
        val index = load(representation.index, 1024 * 1024)
        load(representation.initialization, 1024 * 1024)
        load(VesperDashStartupPlanner.firstMedia(representation.index, index.bytes), VesperDashStartupCache.MAX_RESOURCE_BYTES)
    }
    currentCoroutineContext().ensureActive()
    check(commitFence { cache.store(scope, staged, source.headers, generation, maximumBytes) }) { "DASH startup cache invalidated before commit" }
    return total to allHit
}
