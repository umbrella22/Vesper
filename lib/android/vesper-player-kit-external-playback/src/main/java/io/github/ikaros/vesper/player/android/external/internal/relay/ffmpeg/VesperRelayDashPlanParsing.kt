package io.github.ikaros.vesper.player.android.external.internal.relay.ffmpeg

import java.io.StringReader
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

internal fun dashTemplateFromElement(element: Element): DashTemplate? {
    val template = childElementsByTagName(element, "SegmentTemplate").firstOrNull() ?: return null
    return DashTemplate(
        media = template.getAttribute("media").takeIf(String::isNotBlank) ?: return null,
        initialization = template.getAttribute("initialization").takeIf(String::isNotBlank),
        startNumber = template.getAttribute("startNumber").toLongOrNull() ?: 1L,
        timescale = template.getAttribute("timescale").toLongOrNull() ?: 1L,
        duration = template.getAttribute("duration").toLongOrNull() ?: 0L,
    )
}

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

internal fun parseIso8601DurationSeconds(value: String?): Double? {
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
