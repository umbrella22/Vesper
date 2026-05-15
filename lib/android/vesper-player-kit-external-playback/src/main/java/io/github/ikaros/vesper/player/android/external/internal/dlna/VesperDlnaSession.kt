package io.github.ikaros.vesper.player.android.external.internal.dlna

import io.github.ikaros.vesper.player.android.VesperPlayerSource
import io.github.ikaros.vesper.player.android.VesperSystemPlaybackMetadata

sealed class VesperDlnaOperationResult {
    data object Success : VesperDlnaOperationResult()
    data class Unavailable(val message: String) : VesperDlnaOperationResult()
    data class Unsupported(val message: String) : VesperDlnaOperationResult()
    data class Failed(val message: String) : VesperDlnaOperationResult()
}

class VesperDlnaSession(
    val device: VesperDlnaDevice,
    private val soapClient: VesperDlnaSoapClient = VesperDlnaSoapClient(),
) {
    fun load(
        source: VesperPlayerSource,
        metadata: VesperSystemPlaybackMetadata?,
        startPositionMs: Long = 0,
        autoplay: Boolean = true,
    ): VesperDlnaOperationResult {
        val setUri = soapClient.setAvTransportUri(device, source, metadata).toOperationResult()
        if (setUri !is VesperDlnaOperationResult.Success) {
            return setUri
        }
        if (startPositionMs > 0) {
            val seek = seekTo(startPositionMs)
            if (seek is VesperDlnaOperationResult.Failed) {
                return seek
            }
        }
        return if (autoplay) play() else VesperDlnaOperationResult.Success
    }

    fun play(): VesperDlnaOperationResult =
        soapClient.play(device).toOperationResult()

    fun pause(): VesperDlnaOperationResult =
        soapClient.pause(device).toOperationResult(unsupportedOnFault = true)

    fun stop(): VesperDlnaOperationResult =
        soapClient.stop(device).toOperationResult()

    fun seekTo(positionMs: Long): VesperDlnaOperationResult =
        soapClient.seek(device, positionMs).toOperationResult(unsupportedOnFault = true)

    fun protocolInfo(): String =
        soapClient.getProtocolInfo(device).body
}

private fun VesperDlnaSoapResponse.toOperationResult(
    unsupportedOnFault: Boolean = false,
): VesperDlnaOperationResult {
    if (status == 0) {
        return VesperDlnaOperationResult.Unavailable(body)
    }
    if (status in 200..299 && fault == null) {
        return VesperDlnaOperationResult.Success
    }
    val faultMessage = fault?.description ?: body.takeIf { it.isNotBlank() }
    return if (unsupportedOnFault && fault != null) {
        VesperDlnaOperationResult.Unsupported(faultMessage ?: "DLNA operation is not supported by this device.")
    } else {
        VesperDlnaOperationResult.Failed(faultMessage ?: "DLNA operation failed with HTTP $status.")
    }
}
