package io.github.umbrella22.vesper.player.android

import android.content.Context
import android.content.ContextWrapper
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

class VesperNativeLibraryTest {
    @Test
    fun rejects32BitOnlyDevicesBeforeLoadingNativeCode() {
        val abis = listOf("armeabi-v7a", "armeabi")
        var loadAttempts = 0
        repeat(2) {
            val error = assertThrows(VesperPlayerUnsupportedOperation::class.java) {
                loadVesperNativeLibrary(abis, false) { loadAttempts += 1 }
            }
            assertEquals("unsupportedArchitecture", error.details["reason"])
            assertEquals("arm64-v8a", error.details["requiredAbi"])
            assertEquals(abis, error.details["supportedAbis"])
            assertEquals(false, error.details["is64BitProcess"])
            assertTrue(error.message.orEmpty().contains("armeabi-v7a"))
            assertTrue(error.message.orEmpty().contains("arm64-v8a"))
            assertNull(error.cause)
        }
        assertEquals(0, loadAttempts)
    }

    @Test
    fun rejects32BitProcessesOnArm64Devices() {
        var loaded = false
        val error = assertThrows(VesperPlayerUnsupportedOperation::class.java) {
            loadVesperNativeLibrary(listOf("arm64-v8a", "armeabi-v7a"), false) {
                loaded = true
            }
        }
        assertFalse(loaded)
        assertTrue(error.message.orEmpty().contains("32-bit app process"))
        assertEquals(false, error.details["is64BitProcess"])
    }

    @Test
    fun rejectsIntelAndMissingAbisBeforeLoading() {
        for (abis in listOf(listOf("x86_64", "x86"), emptyList())) {
            val error = assertThrows(VesperPlayerUnsupportedOperation::class.java) {
                loadVesperNativeLibrary(abis, true) {
                    throw AssertionError("unsupported runtimes must not load native code")
                }
            }
            assertEquals(abis, error.details["supportedAbis"])
            assertEquals("unsupportedArchitecture", error.details["reason"])
        }
    }

    @Test
    fun loadsArm64OnDevicesWithOrWithout32BitCompatibility() {
        for (abis in listOf(listOf("arm64-v8a"), listOf("arm64-v8a", "armeabi-v7a"))) {
            val libraries = mutableListOf<String>()
            loadVesperNativeLibrary(abis, true, libraries::add)
            assertEquals(listOf("vesper_player_android"), libraries)
        }
    }

    @Test
    fun preservesNativePackagingErrorsOnSupportedRuntimes() {
        val missingLibrary = UnsatisfiedLinkError("missing arm64 library")
        val error = assertThrows(UnsatisfiedLinkError::class.java) {
            loadVesperNativeLibrary(listOf("arm64-v8a"), true) { throw missingLibrary }
        }
        assertSame(missingLibrary, error)
    }

    @Test
    fun repeatedControllerCreationRejectsBeforeContextOrJniInitialization() {
        // JVM Android stubs report no ABIs and cannot load the Android JNI library.
        val context = object : ContextWrapper(null) {
            override fun getApplicationContext(): Context =
                throw AssertionError("architecture validation must precede player setup")
        }
        repeat(2) {
            val error = assertThrows(VesperPlayerUnsupportedOperation::class.java) {
                VesperPlayerControllerFactory.createDefault(context)
            }
            assertEquals("unsupportedArchitecture", error.details["reason"])
            assertNull(error.cause)
        }
    }
}
