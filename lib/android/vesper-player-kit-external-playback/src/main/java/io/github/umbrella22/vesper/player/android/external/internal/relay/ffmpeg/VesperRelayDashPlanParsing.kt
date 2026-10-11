package io.github.umbrella22.vesper.player.android.external.internal.relay.ffmpeg

import java.io.StringReader
import java.math.BigDecimal
import java.math.RoundingMode
import java.util.Locale
import javax.xml.parsers.DocumentBuilderFactory
import org.w3c.dom.Document
import org.w3c.dom.Element
import org.xml.sax.InputSource

internal fun parseXmlDocument(xmlText: String): Document {
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

internal fun dashTemplateFromElement(
    element: Element,
    baseDetails: Map<String, String> = emptyMap(),
): DashTemplate? {
    val template = childElementsByTagName(element, "SegmentTemplate").firstOrNull() ?: return null
    val timeline = childElementsByTagName(template, "SegmentTimeline")
        .firstOrNull()
        ?.let { timelineElement ->
            val entries = childElementsByTagName(timelineElement, "S")
            if (entries.isEmpty()) {
                throw unsupportedDashLayout(baseDetails, "DASH SegmentTimeline requires at least one S entry.", emptyMap())
            }
            entries.map { entry ->
                val repeatCount = if (entry.hasAttribute("r")) {
                    entry.getAttribute("r").toIntOrNull()?.takeIf { it >= -1 }
                        ?: throw invalidDashTemplateAttribute("SegmentTimeline S@r", baseDetails)
                } else {
                    0
                }
                DashTimelineEntry(
                    startTime = entry.dashLongAttribute("t", "SegmentTimeline S@t", baseDetails),
                    duration = entry.dashLongAttribute("d", "SegmentTimeline S@d", baseDetails, minimum = 1L)
                        ?: throw invalidDashTemplateAttribute("SegmentTimeline S@d", baseDetails),
                    repeatCount = repeatCount,
                )
            }
        }
        ?: emptyList()
    return DashTemplate(
        media = template.getAttribute("media").takeIf(String::isNotBlank)
            ?: throw invalidDashTemplateAttribute("SegmentTemplate media", baseDetails),
        initialization = template.getAttribute("initialization").takeIf(String::isNotBlank),
        startNumber = template.dashLongAttribute("startNumber", "SegmentTemplate startNumber", baseDetails) ?: 1L,
        timescale = template.dashLongAttribute("timescale", "SegmentTemplate timescale", baseDetails, minimum = 1L) ?: 1L,
        duration = template.dashLongAttribute("duration", "SegmentTemplate duration", baseDetails, minimum = 1L),
        presentationTimeOffset = template.dashLongAttribute("presentationTimeOffset", "SegmentTemplate presentationTimeOffset", baseDetails) ?: 0L,
        timeline = timeline,
    )
}

internal fun Element.dashLongAttribute(
    attribute: String,
    field: String,
    baseDetails: Map<String, String>,
    minimum: Long = 0L,
): Long? {
    if (!hasAttribute(attribute)) return null
    return getAttribute(attribute).toLongOrNull()?.takeIf { it >= minimum }
        ?: throw invalidDashTemplateAttribute(field, baseDetails)
}

private fun invalidDashTemplateAttribute(field: String, baseDetails: Map<String, String>): VesperRelayHostInputException =
    unsupportedDashLayout(
        baseDetails = baseDetails,
        message = "$field is invalid for host-prepared relay remux.",
        details = mapOf("field" to field),
    )

internal fun dashSegmentBaseFromElement(
    element: Element,
    baseDetails: Map<String, String>,
    kind: String,
    mediaId: String,
): DashSegmentBase? {
    val segmentBase = childElementsByTagName(element, "SegmentBase").firstOrNull() ?: return null
    val indexRangeValue = segmentBase.getAttribute("indexRange").takeIf(String::isNotBlank)
        ?: throw unsupportedDashLayout(
            baseDetails = baseDetails,
            message = "DASH SegmentBase requires an indexRange.",
            details = mapOf("trackKind" to kind, "mediaId" to mediaId),
        )
    val initializationRangeValue = childElementsByTagName(segmentBase, "Initialization")
        .firstOrNull()
        ?.getAttribute("range")
        ?.takeIf(String::isNotBlank)
        ?: throw unsupportedDashLayout(
            baseDetails = baseDetails,
            message = "DASH SegmentBase requires an Initialization range.",
            details = mapOf("trackKind" to kind, "mediaId" to mediaId),
        )
    val indexRange = parseDashByteRange(indexRangeValue, "SegmentBase indexRange", baseDetails, kind, mediaId)
    val initializationRange = parseDashByteRange(
        initializationRangeValue,
        "SegmentBase Initialization range",
        baseDetails,
        kind,
        mediaId,
    )
    return DashSegmentBase(
        initialization = initializationRange,
        indexRange = indexRange,
    )
}

private fun parseDashByteRange(
    value: String,
    field: String,
    baseDetails: Map<String, String>,
    kind: String,
    mediaId: String,
): VesperRelayDashByteRange {
    val separator = value.indexOf('-')
    if (separator <= 0 || separator == value.lastIndex) {
        throw invalidDashByteRange(value, field, baseDetails, kind, mediaId)
    }
    val startText = value.substring(0, separator)
    val endText = value.substring(separator + 1)
    val start = startText.trim().toLongOrNull()
        ?: throw invalidDashByteRange(value, field, baseDetails, kind, mediaId)
    val end = endText.trim().toLongOrNull()
        ?: throw invalidDashByteRange(value, field, baseDetails, kind, mediaId)
    if (start < 0L || end < start) {
        throw invalidDashByteRange(value, field, baseDetails, kind, mediaId)
    }
    return VesperRelayDashByteRange(start = start, end = end)
}

private fun invalidDashByteRange(
    value: String,
    field: String,
    baseDetails: Map<String, String>,
    kind: String,
    mediaId: String,
): VesperRelayHostInputException =
    unsupportedDashLayout(
        baseDetails = baseDetails,
        message = "$field is invalid for host-prepared relay remux.",
        details = mapOf("trackKind" to kind, "mediaId" to mediaId, "byteRange" to value),
    )

internal fun dashMediaKind(
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

internal fun childElementsByTagName(
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

internal fun Document.hasDashElement(tagName: String): Boolean =
    getElementsByTagNameNS("*", tagName).length > 0 || getElementsByTagName(tagName).length > 0

internal fun firstBaseUrl(element: Element): String? =
    childElementsByTagName(element, "BaseURL")
        .firstOrNull()
        ?.textContent
        ?.trim()
        ?.takeIf { it.isNotEmpty() }

internal fun parseIso8601DurationMilliseconds(value: String?): Long? {
    val match = DASH_DURATION_PATTERN.matchEntire(value ?: return null) ?: return null
    val multipliers = listOf(86_400_000L, 3_600_000L, 60_000L, 1_000L)
    val total = multipliers.mapIndexed { index, multiplier ->
        (match.groupValues[index + 1].toBigDecimalOrNull() ?: BigDecimal.ZERO) * multiplier.toBigDecimal()
    }.fold(BigDecimal.ZERO, BigDecimal::add)
    return runCatching {
        total.setScale(0, RoundingMode.HALF_UP).longValueExact()
    }.getOrNull()?.takeIf { it > 0L }
}

private val DASH_DURATION_PATTERN = Regex(
    """P(?:(\d+(?:\.\d+)?)D)?(?:T(?:(\d+(?:\.\d+)?)H)?(?:(\d+(?:\.\d+)?)M)?(?:(\d+(?:\.\d+)?)S)?)?""",
)
