package io.github.ikaros.vesper.player.android.external.internal.relay.ffmpeg

import java.io.InputStream
import java.io.IOException

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
        val read = native.read(handle, buffer, offset, length)
        return when (read) {
            NATIVE_READ_COMPLETE -> -1
            NATIVE_READ_INVALID_HANDLE -> throw VesperRelayIOException(
                code = "native_stream_invalid",
                message = "Native FFmpeg relay stream handle is invalid.",
            )
            NATIVE_READ_FAILED -> throw VesperRelayIOException(
                code = "native_stream_failed",
                message = "Native FFmpeg relay stream failed before completion.",
            )
            NATIVE_READ_STALLED -> throw VesperRelayIOException(
                code = "native_stream_stalled",
                message = "Native FFmpeg relay stream made no progress before its deadline.",
            )
            NATIVE_READ_CANCELLED -> throw VesperRelayIOException(
                code = "native_stream_cancelled",
                message = "Native FFmpeg relay stream was cancelled.",
            )
            in Int.MIN_VALUE until 0 -> throw VesperRelayIOException(
                code = "native_stream_protocol_error",
                message = "Native FFmpeg relay stream returned unknown outcome $read.",
            )
            else -> read
        }
    }

    override fun close() {
        val current = handle
        handle = 0
        if (current != 0L) {
            native.close(current)
        }
    }
}

internal class VesperRelayIOException(
    val code: String,
    message: String,
) : IOException("[$code] $message")

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
            if (loaded) {
                return
            }
        }
        // Load the native library outside the synchronized block to avoid
        // holding a monitor during file I/O (AGENTS.md rule). A second thread
        // may also pass through, but System.loadLibrary is idempotent.
        System.loadLibrary("vesper_player_relay_ffmpeg")
        loaded = true
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

private const val NATIVE_READ_COMPLETE = -1
private const val NATIVE_READ_INVALID_HANDLE = -2
private const val NATIVE_READ_FAILED = -3
private const val NATIVE_READ_STALLED = -4
private const val NATIVE_READ_CANCELLED = -5
