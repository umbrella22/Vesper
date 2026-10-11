package io.github.umbrella22.vesper.player.flutter.android

import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.withContext

internal suspend fun <Manager : Any, Session> createAndRegisterDownloadManager(
    createManager: () -> Manager,
    registerManager: (Manager) -> Session,
    disposeManager: (Manager) -> Unit,
    ioDispatcher: CoroutineDispatcher = Dispatchers.IO,
): Session {
    var ownedManager: Manager? = null
    try {
        withContext(ioDispatcher) {
            // Keep ownership outside the cancellable return to the caller.
            ownedManager = createManager()
        }
        val session = registerManager(checkNotNull(ownedManager))
        ownedManager = null
        return session
    } finally {
        ownedManager?.let { manager ->
            withContext(NonCancellable + ioDispatcher) {
                disposeManager(manager)
            }
        }
    }
}
