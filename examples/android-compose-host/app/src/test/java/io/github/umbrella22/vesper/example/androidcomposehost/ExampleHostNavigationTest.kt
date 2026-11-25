package io.github.umbrella22.vesper.example.androidcomposehost

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class ExampleHostNavigationTest {
    @Test
    fun backClosesSheetBeforeLeavingCollection() {
        val state =
            ExampleHostNavState()
                .openCollection(ExampleLibraryCollection.DolbyAcceptance)
                .openSheet(ExampleHostSheet.OpenMedia)

        val afterSheet = state.back()
        assertEquals(null, afterSheet?.sheet)
        assertEquals(ExampleLibraryCollection.DolbyAcceptance, afterSheet?.libraryCollection)

        val afterCollection = afterSheet?.back()
        assertEquals(ExampleHostDestination.Library, afterCollection?.destination)
        assertNull(afterCollection?.libraryCollection)

        val afterLibrary = afterCollection?.back()
        assertEquals(ExampleHostDestination.Play, afterLibrary?.destination)
        assertNull(afterLibrary?.back())
    }

    @Test
    fun playRootLeavesBackToSystem() {
        val state = ExampleHostNavState()
        assertFalse(state.canHandleBack)
        assertNull(state.back())
        assertTrue(state.openSheet(ExampleHostSheet.Settings).canHandleBack)
    }

    @Test
    fun reselectingLibraryPopsToLibraryHome() {
        val state = ExampleHostNavState().openCollection(ExampleLibraryCollection.Live)
        val reselected = state.select(ExampleHostDestination.Library)
        assertEquals(ExampleHostDestination.Library, reselected.destination)
        assertNull(reselected.libraryCollection)
    }

    @Test
    fun switchingTabsKeepsOpenCollectionAndDropsSheet() {
        val state =
            ExampleHostNavState()
                .openCollection(ExampleLibraryCollection.Adaptive)
                .openSheet(ExampleHostSheet.Settings)
        val lab = state.select(ExampleHostDestination.Lab)
        assertEquals(ExampleHostDestination.Lab, lab.destination)
        assertNull(lab.sheet)
        val library = lab.select(ExampleHostDestination.Library)
        assertEquals(ExampleLibraryCollection.Adaptive, library.libraryCollection)
    }

    @Test
    fun pendingRemovalHidesTaskUntilUndoOrCommit() {
        val tasks = listOf(1L, 2L, 3L)
        val staged = ExamplePendingRemovals().stage(2L)
        assertEquals(listOf(1L, 3L), staged.visible(tasks) { it })
        assertEquals(tasks, staged.undo(2L).visible(tasks) { it })
        assertTrue(staged.commit(2L).pendingIds.isEmpty())
    }
}
