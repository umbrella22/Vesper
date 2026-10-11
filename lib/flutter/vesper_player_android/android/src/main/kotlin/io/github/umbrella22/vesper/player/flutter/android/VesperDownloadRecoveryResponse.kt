package io.github.umbrella22.vesper.player.flutter.android

import io.github.umbrella22.vesper.player.android.VesperDownloadRecoveredTaskPlan
import io.flutter.plugin.common.MethodChannel
import kotlinx.coroutines.CompletableDeferred

internal class DownloadRecoveryChannelException(
    val errorCode: String,
    val errorMessage: String?,
    val errorDetails: Any?,
) : RuntimeException("Download recovery failed ($errorCode): ${errorMessage.orEmpty()}")

internal class DownloadRecoveryResult(
    private val deferred: CompletableDeferred<VesperDownloadRecoveredTaskPlan?>,
) : MethodChannel.Result {
    override fun success(result: Any?) {
        completeDownloadRecoveryResponse(result, deferred)
    }

    override fun error(errorCode: String, errorMessage: String?, errorDetails: Any?) {
        deferred.completeExceptionally(DownloadRecoveryChannelException(errorCode, errorMessage, errorDetails))
    }

    override fun notImplemented() {
        deferred.complete(null)
    }
}

internal fun completeDownloadRecoveryResponse(
    response: Any?,
    deferred: CompletableDeferred<VesperDownloadRecoveredTaskPlan?>,
) {
    try {
        val plan = mapOf("plan" to response).optionalInputMap("plan")
            ?.toDownloadRecoveredTaskPlan()
        deferred.complete(plan)
    } catch (error: IllegalArgumentException) {
        // Return malformed plans to the awaiting download operation, not the channel callback.
        deferred.completeExceptionally(error)
    }
}
