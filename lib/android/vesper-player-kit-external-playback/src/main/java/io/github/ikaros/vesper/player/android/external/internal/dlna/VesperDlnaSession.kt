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
    ): VesperDlnaOperationResult =
        loadWithOperations(
            startPositionMs = startPositionMs,
            autoplay = autoplay,
            setUri = { soapClient.setAvTransportUri(device, source, metadata).toOperationResult() },
            seek = ::seekTo,
            play = ::play,
        )

    suspend fun loadAsync(
        source: VesperPlayerSource,
        metadata: VesperSystemPlaybackMetadata?,
        startPositionMs: Long = 0,
        autoplay: Boolean = true,
    ): VesperDlnaOperationResult =
        loadWithOperationsAsync(
            startPositionMs = startPositionMs,
            autoplay = autoplay,
            setUri = { soapClient.setAvTransportUriAsync(device, source, metadata).toOperationResult() },
            seek = ::seekToAsync,
            play = ::playAsync,
        )

    fun play(): VesperDlnaOperationResult =
        soapClient.play(device).toOperationResult()

    suspend fun playAsync(): VesperDlnaOperationResult =
        soapClient.playAsync(device).toOperationResult()

    fun pause(): VesperDlnaOperationResult =
        soapClient.pause(device).toOperationResult(unsupportedOnFault = true)

    suspend fun pauseAsync(): VesperDlnaOperationResult =
        soapClient.pauseAsync(device).toOperationResult(unsupportedOnFault = true)

    fun stop(): VesperDlnaOperationResult =
        soapClient.stop(device).toOperationResult()

    suspend fun stopAsync(): VesperDlnaOperationResult =
        soapClient.stopAsync(device).toOperationResult()

    fun seekTo(positionMs: Long): VesperDlnaOperationResult =
        soapClient.seek(device, positionMs).toOperationResult(unsupportedOnFault = true)

    suspend fun seekToAsync(positionMs: Long): VesperDlnaOperationResult =
        soapClient.seekAsync(device, positionMs).toOperationResult(unsupportedOnFault = true)

    fun protocolInfo(): String =
        soapClient.getProtocolInfo(device).body

    suspend fun protocolInfoAsync(): String =
        soapClient.getProtocolInfoAsync(device).body

    private fun loadWithOperations(
        startPositionMs: Long,
        autoplay: Boolean,
        setUri: () -> VesperDlnaOperationResult,
        seek: (Long) -> VesperDlnaOperationResult,
        play: () -> VesperDlnaOperationResult,
    ): VesperDlnaOperationResult {
        val setUriResult = setUri()
        if (setUriResult !is VesperDlnaOperationResult.Success) {
            return setUriResult
        }
        if (startPositionMs > 0) {
            val seekResult = seek(startPositionMs)
            if (seekResult is VesperDlnaOperationResult.Failed) {
                return seekResult
            }
        }
        return if (autoplay) play() else VesperDlnaOperationResult.Success
    }

    private suspend fun loadWithOperationsAsync(
        startPositionMs: Long,
        autoplay: Boolean,
        setUri: suspend () -> VesperDlnaOperationResult,
        seek: suspend (Long) -> VesperDlnaOperationResult,
        play: suspend () -> VesperDlnaOperationResult,
    ): VesperDlnaOperationResult {
        val setUriResult = setUri()
        if (setUriResult !is VesperDlnaOperationResult.Success) {
            return setUriResult
        }
        if (startPositionMs > 0) {
            val seekResult = seek(startPositionMs)
            if (seekResult is VesperDlnaOperationResult.Failed) {
                return seekResult
            }
        }
        return if (autoplay) play() else VesperDlnaOperationResult.Success
    }
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
