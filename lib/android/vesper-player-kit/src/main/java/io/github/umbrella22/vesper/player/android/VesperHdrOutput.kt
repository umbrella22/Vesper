package io.github.umbrella22.vesper.player.android

import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

/** Observed display output. Capability and decoder metadata cannot confirm this state. */
enum class VesperHdrOutputState(val wireName: String) {
    Unknown("unknown"), Sdr("sdr"), Hdr("hdr"),
}

enum class VesperHdrOutputFormat(val wireName: String) {
    Unknown("unknown"), Hdr10("hdr10"), Hlg("hlg"), DolbyVision("dolbyVision"),
}

/**
 * Output evidence and its native lifecycle context. Revisions are local to one
 * controller; sourceRevision counts source activations, including repeated URLs.
 * Android currently reports unknown because no per-Surface output observer is enabled.
 */
@ConsistentCopyVisibility
data class VesperHdrOutputSnapshot internal constructor(
    val state: VesperHdrOutputState = VesperHdrOutputState.Unknown,
    val format: VesperHdrOutputFormat = VesperHdrOutputFormat.Unknown,
    val sourceRevision: Long = 0,
    val outputGeneration: Long = 0,
    val effectiveVideoTrackId: String? = null,
    val catalogRevision: Long? = null,
    val displayId: String? = null,
    val evidence: String? = null,
    val reason: String? = "outputObservationUnavailable",
)

internal data class VesperHdrOutputObservationToken(
    val owner: Any,
    val sourceRevision: Long,
    val outputGeneration: Long,
)

internal data class VesperHdrOutputObservation(
    val state: VesperHdrOutputState,
    val format: VesperHdrOutputFormat = VesperHdrOutputFormat.Unknown,
    val evidence: String? = null,
    val reason: String? = null,
)

/** Mutated on the player looper. Tokens belong to this instance, never to recurring IDs. */
internal class VesperHdrOutputTracker(hasInitialSource: Boolean = false) {
    private val owner = Any()
    private var disposed = false
    private var outputListener: ((VesperHdrOutputSnapshot) -> Unit)? = null
    private val mutableSnapshot = MutableStateFlow(
        VesperHdrOutputSnapshot(
            sourceRevision = if (hasInitialSource) 1 else 0,
            outputGeneration = if (hasInitialSource) 1 else 0,
        ),
    )
    val snapshot: StateFlow<VesperHdrOutputSnapshot> = mutableSnapshot.asStateFlow()

    fun setListener(listener: ((VesperHdrOutputSnapshot) -> Unit)?) {
        if (disposed) return
        outputListener = listener
        listener?.invoke(snapshot.value)
    }

    private fun publish(next: VesperHdrOutputSnapshot) {
        if (next == snapshot.value) return
        mutableSnapshot.value = next
        // StateFlow may conflate transitions; the boundary listener must see each one.
        outputListener?.invoke(next)
    }

    fun capture(): VesperHdrOutputObservationToken = snapshot.value.let {
        VesperHdrOutputObservationToken(owner, it.sourceRevision, it.outputGeneration)
    }

    fun sourceChanged() {
        if (disposed) return
        invalidate(snapshot.value.copy(
            sourceRevision = snapshot.value.sourceRevision + 1,
            effectiveVideoTrackId = null,
            catalogRevision = null,
        ))
    }

    fun videoTrackChanged(trackId: String?, catalogRevision: Long?) {
        val current = snapshot.value
        if (current.effectiveVideoTrackId == trackId && current.catalogRevision == catalogRevision) return
        invalidate(current.copy(effectiveVideoTrackId = trackId, catalogRevision = catalogRevision))
    }

    fun outputPathChanged(displayId: String? = snapshot.value.displayId) {
        invalidate(snapshot.value.copy(displayId = displayId))
    }

    private fun invalidate(context: VesperHdrOutputSnapshot) {
        if (disposed) return
        publish(context.copy(
            outputGeneration = snapshot.value.outputGeneration + 1,
            state = VesperHdrOutputState.Unknown,
            format = VesperHdrOutputFormat.Unknown,
            evidence = null,
            reason = "outputObservationUnavailable",
        ))
    }

    fun apply(token: VesperHdrOutputObservationToken, observation: VesperHdrOutputObservation): Boolean {
        val current = snapshot.value
        if (disposed || token.owner !== owner || token.sourceRevision != current.sourceRevision ||
            token.outputGeneration != current.outputGeneration
        ) return false
        if (observation.state != VesperHdrOutputState.Unknown && observation.evidence.isNullOrBlank()) return false
        publish(current.copy(
            state = observation.state,
            format = if (observation.state == VesperHdrOutputState.Hdr) observation.format else VesperHdrOutputFormat.Unknown,
            evidence = observation.evidence.takeIf { observation.state != VesperHdrOutputState.Unknown },
            reason = observation.reason,
        ))
        return true
    }

    fun dispose() {
        if (disposed) return
        outputPathChanged(displayId = null)
        disposed = true
        outputListener = null
    }
}
