package io.github.umbrella22.vesper.player.android

import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

/** Native video evidence. Neither observation proves that application UI is visible. */
enum class VesperFirstFrameObservationKind(val wireName: String) {
    Media3RenderedFirstFrame("media3RenderedFirstFrame"),
    AvPlayerLayerReadyForDisplay("avPlayerLayerReadyForDisplay"),
}

/** Startup elapsed time is monotonic and independent of the media position. */
data class VesperFirstFrameObservation(
    val playbackEpoch: Long,
    val elapsedSinceLoadStartMs: Long,
    val kind: VesperFirstFrameObservationKind,
    val mediaPositionMs: Long? = null,
) {
    fun toMap(): Map<String, Any?> = mapOf(
        "playbackEpoch" to playbackEpoch,
        "elapsedSinceLoadStartMs" to elapsedSinceLoadStartMs,
        "kind" to kind.wireName,
        "mediaPositionMs" to mediaPositionMs,
    )
}

enum class VesperAudioDiagnosticEvidence(val wireName: String) {
    Unknown("unknown"), RuntimeFormat("runtimeFormat"),
    SelectedMediaOption("selectedMediaOption"), ManifestMetadata("manifestMetadata"),
}

enum class VesperAudioDiagnosticIssueKind(val wireName: String) {
    DecoderError("decoderError"), SinkError("sinkError"),
}

/** A possibly recoverable platform callback; it does not itself terminate playback. */
data class VesperAudioDiagnosticIssue(
    val kind: VesperAudioDiagnosticIssueKind,
    val elapsedSinceLoadStartMs: Long,
    val platformCode: String? = null,
    val message: String? = null,
) {
    fun toMap(): Map<String, Any?> = mapOf(
        "kind" to kind.wireName,
        "elapsedSinceLoadStartMs" to elapsedSinceLoadStartMs,
        "platformCode" to platformCode,
        "message" to message,
    )
}

/** Selected input evidence. Decoder availability never confirms audio output progress. */
data class VesperAudioPlaybackDiagnostics(
    val trackId: String? = null,
    val formatId: String? = null,
    val codec: String? = null,
    val sampleMimeType: String? = null,
    val decoderName: String? = null,
    val channels: Int? = null,
    val sampleRate: Int? = null,
    val evidence: VesperAudioDiagnosticEvidence = VesperAudioDiagnosticEvidence.Unknown,
    val lastIssue: VesperAudioDiagnosticIssue? = null,
) {
    fun toMap(): Map<String, Any?> = mapOf(
        "trackId" to trackId, "formatId" to formatId, "codec" to codec,
        "sampleMimeType" to sampleMimeType, "decoderName" to decoderName,
        "channels" to channels, "sampleRate" to sampleRate,
        "evidence" to evidence.wireName, "lastIssue" to lastIssue?.toMap(),
    )
}

/** Retained evidence for a controller-local native load attempt; zero means no attempt. */
data class VesperPlaybackDiagnosticsSnapshot(
    val playbackEpoch: Long = 0,
    val audio: VesperAudioPlaybackDiagnostics = VesperAudioPlaybackDiagnostics(),
    val firstFrame: VesperFirstFrameObservation? = null,
    val lastStall: VesperPlaybackStallObservation? = null,
) {
    fun toMap(): Map<String, Any?> = mapOf(
        "playbackEpoch" to playbackEpoch,
        "audio" to audio.toMap(),
        "firstFrame" to firstFrame?.toMap(),
        "lastStall" to lastStall?.toMap(),
    )
}

internal data class VesperPlaybackObservationToken(val owner: Any, val epoch: Long)

/** Main-looper state. Each new load, including reloads, receives a fresh token. */
internal class VesperPlaybackDiagnosticsTracker(
    private val nowMs: () -> Long = { android.os.SystemClock.elapsedRealtime() },
) {
    internal val stallDetector = VesperPlaybackStallDetector()
    private val owner = Any()
    private var epoch = 0L
    private var startedAtMs: Long? = null
    private var disposed = false
    private var listener: ((VesperPlaybackDiagnosticsSnapshot) -> Unit)? = null
    private val mutableSnapshot = MutableStateFlow(VesperPlaybackDiagnosticsSnapshot())
    val snapshot: StateFlow<VesperPlaybackDiagnosticsSnapshot> = mutableSnapshot.asStateFlow()

    fun setListener(value: ((VesperPlaybackDiagnosticsSnapshot) -> Unit)?) {
        if (disposed) return
        listener = value
        value?.invoke(snapshot.value)
    }

    fun beginAttempt(): VesperPlaybackObservationToken {
        if (!disposed) {
            stallDetector.resetAttempt()
            epoch += 1
            startedAtMs = nowMs()
            val token = capture()
            publish(VesperPlaybackDiagnosticsSnapshot(playbackEpoch = epoch))
            return token
        }
        return capture()
    }

    /** Immediately rejects old evidence while the replacement load is being scheduled. */
    fun invalidate() {
        if (disposed) return
        stallDetector.resetAttempt()
        epoch += 1
        startedAtMs = null
        publish(VesperPlaybackDiagnosticsSnapshot(playbackEpoch = epoch))
    }

    fun capture() = VesperPlaybackObservationToken(owner, epoch)

    fun isCurrent(token: VesperPlaybackObservationToken): Boolean =
        !disposed && startedAtMs != null && token.owner === owner && token.epoch == epoch

    fun firstFrame(token: VesperPlaybackObservationToken, mediaPositionMs: Long?): Boolean {
        if (!isCurrent(token) || snapshot.value.firstFrame != null) return false
        publish(snapshot.value.copy(firstFrame = VesperFirstFrameObservation(
            playbackEpoch = epoch,
            elapsedSinceLoadStartMs = elapsedMs(),
            kind = VesperFirstFrameObservationKind.Media3RenderedFirstFrame,
            mediaPositionMs = mediaPositionMs,
        )))
        return true
    }

    fun audioFormat(token: VesperPlaybackObservationToken, value: VesperAudioPlaybackDiagnostics) {
        if (!isCurrent(token)) return
        publish(snapshot.value.copy(audio = value.copy(
            decoderName = snapshot.value.audio.decoderName,
            lastIssue = snapshot.value.audio.lastIssue,
        )))
    }

    fun audioDecoder(token: VesperPlaybackObservationToken, name: String?) {
        if (!isCurrent(token)) return
        publish(snapshot.value.copy(audio = snapshot.value.audio.copy(decoderName = name)))
    }

    fun audioDisabled(token: VesperPlaybackObservationToken) {
        if (!isCurrent(token)) return
        publish(snapshot.value.copy(audio = VesperAudioPlaybackDiagnostics(lastIssue = snapshot.value.audio.lastIssue)))
    }

    fun audioIssue(
        token: VesperPlaybackObservationToken,
        kind: VesperAudioDiagnosticIssueKind,
        platformCode: String?,
        message: String?,
    ): VesperPlaybackDiagnosticsSnapshot? {
        if (!isCurrent(token)) return null
        val captured = snapshot.value.copy(audio = snapshot.value.audio.copy(lastIssue = VesperAudioDiagnosticIssue(
            kind, elapsedMs(), platformCode, message,
        )))
        publish(captured)
        return captured
    }

    fun sampleStall(
        token: VesperPlaybackObservationToken, positionMs: Long?, eligible: Boolean, buffering: Boolean,
    ): VesperPlaybackStallObservation? {
        if (!isCurrent(token)) return null
        val evidence = stallDetector.sample(nowMs(), positionMs, eligible, buffering) ?: return null
        val captured = VesperPlaybackStallObservation(
            epoch, evidence.kind, evidence.durationMs, elapsedMs(), positionMs!!, snapshot.value.audio,
        )
        publish(snapshot.value.copy(lastStall = captured))
        return captured
    }

    fun dispose() {
        if (disposed) return
        listener = null
        startedAtMs = null
        disposed = true
    }

    private fun elapsedMs(): Long = (nowMs() - (startedAtMs ?: nowMs())).coerceAtLeast(0L)

    private fun publish(value: VesperPlaybackDiagnosticsSnapshot) {
        if (value == snapshot.value) return
        mutableSnapshot.value = value
        listener?.invoke(value)
    }
}
