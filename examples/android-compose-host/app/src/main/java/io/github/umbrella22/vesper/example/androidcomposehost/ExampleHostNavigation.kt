package io.github.umbrella22.vesper.example.androidcomposehost

import androidx.annotation.StringRes
import androidx.compose.runtime.saveable.Saver
import androidx.compose.runtime.saveable.listSaver

/** Top-level destinations of the example host shell. */
internal enum class ExampleHostDestination(@get:StringRes val labelRes: Int) {
    Play(R.string.example_nav_play),
    Library(R.string.example_nav_library),
    Downloads(R.string.example_nav_downloads),
    Lab(R.string.example_nav_lab),
}

/** Collections shown on the library home; each one opens a secondary page. */
internal enum class ExampleLibraryCollection(
    @get:StringRes val titleRes: Int,
    @get:StringRes val descriptionRes: Int,
) {
    Adaptive(R.string.example_library_streams_title, R.string.example_library_streams_desc),
    Live(R.string.example_library_live_title, R.string.example_library_live_desc),
    DolbyAcceptance(R.string.example_library_dolby_title, R.string.example_library_dolby_desc),
}

/** Host-owned bottom sheets (the SDK stage sheets are tracked separately). */
internal enum class ExampleHostSheet {
    OpenMedia,
    Settings,
}

/**
 * Immutable navigation state of the host shell.
 *
 * Back handling follows the prototype: close a host sheet first, then leave the
 * open library collection, then return to the Play destination. Only at the Play
 * root is back left to the system.
 */
internal data class ExampleHostNavState(
    val destination: ExampleHostDestination = ExampleHostDestination.Play,
    val libraryCollection: ExampleLibraryCollection? = null,
    val sheet: ExampleHostSheet? = null,
) {
    val canHandleBack: Boolean
        get() =
            sheet != null ||
                (destination == ExampleHostDestination.Library && libraryCollection != null) ||
                destination != ExampleHostDestination.Play

    /** Returns the state after a back press, or `null` when the system should handle it. */
    fun back(): ExampleHostNavState? =
        when {
            sheet != null -> copy(sheet = null)
            destination == ExampleHostDestination.Library && libraryCollection != null ->
                copy(libraryCollection = null)
            destination != ExampleHostDestination.Play ->
                copy(destination = ExampleHostDestination.Play)
            else -> null
        }

    /**
     * Selects a destination. Re-selecting the library while a collection is open pops
     * back to the library home, matching the platform re-select behavior.
     */
    fun select(next: ExampleHostDestination): ExampleHostNavState =
        when {
            next == destination && next == ExampleHostDestination.Library ->
                copy(libraryCollection = null, sheet = null)
            else -> copy(destination = next, sheet = null)
        }

    fun openCollection(collection: ExampleLibraryCollection): ExampleHostNavState =
        copy(destination = ExampleHostDestination.Library, libraryCollection = collection, sheet = null)

    fun openSheet(next: ExampleHostSheet): ExampleHostNavState = copy(sheet = next)

    fun dismissSheet(): ExampleHostNavState = copy(sheet = null)

    companion object {
        val Saver: Saver<ExampleHostNavState, Any> =
            listSaver(
                save = { state ->
                    listOf(
                        state.destination.name,
                        state.libraryCollection?.name.orEmpty(),
                        state.sheet?.name.orEmpty(),
                    )
                },
                restore = { values ->
                    ExampleHostNavState(
                        destination = ExampleHostDestination.valueOf(values[0]),
                        libraryCollection =
                            values[1].takeIf(String::isNotEmpty)?.let(ExampleLibraryCollection::valueOf),
                        sheet = values[2].takeIf(String::isNotEmpty)?.let(ExampleHostSheet::valueOf),
                    )
                },
            )
    }
}

/**
 * Download removals are deferred so the snackbar can offer an undo. Task ids in
 * [pendingIds] are hidden from the list until the removal is committed or undone.
 */
internal data class ExamplePendingRemovals(val pendingIds: Set<Long> = emptySet()) {
    fun stage(taskId: Long): ExamplePendingRemovals = copy(pendingIds = pendingIds + taskId)

    fun undo(taskId: Long): ExamplePendingRemovals = copy(pendingIds = pendingIds - taskId)

    fun commit(taskId: Long): ExamplePendingRemovals = copy(pendingIds = pendingIds - taskId)

    fun <T> visible(items: List<T>, idOf: (T) -> Long): List<T> =
        if (pendingIds.isEmpty()) items else items.filterNot { idOf(it) in pendingIds }
}
