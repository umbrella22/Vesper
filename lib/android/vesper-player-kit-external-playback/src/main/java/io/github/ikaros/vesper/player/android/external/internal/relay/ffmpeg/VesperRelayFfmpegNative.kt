package io.github.ikaros.vesper.player.android.external.internal.relay.ffmpeg

import java.io.InputStream

data class VesperRelayFfmpegOpenResult(
    val handle: Long,
    val status: Int,
    val contentType: String,
    val contentLength: Long,
    val headers: Map<String, String>,
    val errorCode: String?,
    val errorMessage: String?,
    val errorDetails: Map<String, String>,
)

internal class VesperRelayFfmpegInputStream(
    private var handle: Long,
    private val native: VesperRelayFfmpegNativeApi = VesperRelayFfmpegNativeBridge,
) : InputStream() {
    override fun read(): Int {
        val buffer = ByteArray(1)
        val read = read(buffer, 0, 1)
        return if (read <= 0) -1 else buffer[0].toInt() and 0xff
    }

    override fun read(buffer: ByteArray, offset: Int, length: Int): Int {
        if (offset < 0 || length < 0 || length > buffer.size - offset) {
            throw IndexOutOfBoundsException(
                "offset=$offset length=$length bufferSize=${buffer.size}",
            )
        }
        if (length == 0) {
            return 0
        }
        if (handle == 0L) {
            return -1
        }
        return native.read(handle, buffer, offset, length)
    }

    override fun close() {
        val current = handle
        handle = 0
        if (current != 0L) {
            native.close(current)
        }
    }
}

internal interface VesperRelayFfmpegNativeApi {
    fun read(handle: Long, buffer: ByteArray, offset: Int, length: Int): Int

    fun close(handle: Long)
}

internal object VesperRelayFfmpegNative {
    @Volatile
    private var loaded = false

    fun ensureLoaded() {
        if (loaded) {
            return
        }
        synchronized(this) {
            if (!loaded) {
                System.loadLibrary("vesper_player_relay_ffmpeg")
                loaded = true
            }
        }
    }

    @JvmStatic
    external fun runtimeMetadata(): String

    @JvmStatic
    external fun open(requestJson: String): VesperRelayFfmpegOpenResult

    @JvmStatic
    external fun prewarm(requestJson: String): VesperRelayFfmpegOpenResult

    @JvmStatic
    external fun read(handle: Long, buffer: ByteArray, offset: Int, length: Int): Int

    @JvmStatic
    external fun close(handle: Long)

    @JvmStatic
    external fun invalidate(sessionId: String)
}

private object VesperRelayFfmpegNativeBridge : VesperRelayFfmpegNativeApi {
    override fun read(handle: Long, buffer: ByteArray, offset: Int, length: Int): Int =
        VesperRelayFfmpegNative.read(handle, buffer, offset, length)

    override fun close(handle: Long) {
        VesperRelayFfmpegNative.close(handle)
    }
}
