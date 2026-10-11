package io.github.umbrella22.vesper.player.flutter.android

import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicReference
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.coroutines.async
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeout
import org.junit.Assert.assertEquals
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test

class VesperDownloadManagerCreationTest {
    @Test
    fun engineDetachDuringConstructionDisposesUnregisteredManager() = runBlocking {
        Fixture().use { fixture ->
            val constructed = CountDownLatch(1)
            val returnFromConstruction = CountDownLatch(1)
            val manager = AtomicReference<Manager>()
            val creation = fixture.scope.launch {
                createAndRegisterDownloadManager(
                    createManager = {
                        Manager().also {
                            manager.set(it)
                            constructed.countDown()
                            check(returnFromConstruction.await(5, TimeUnit.SECONDS))
                        }
                    },
                    registerManager = fixture::register,
                    disposeManager = Manager::dispose,
                    ioDispatcher = fixture.ioDispatcher,
                )
            }
            try {
                assertTrue(constructed.await(5, TimeUnit.SECONDS))
                withContext(fixture.mainDispatcher) { fixture.detach() }
                returnFromConstruction.countDown()
                withTimeout(5_000) { creation.join() }

                assertTrue(creation.isCancelled)
                assertTrue(fixture.registered.isEmpty())
                assertEquals(1, manager.get().disposeCount.get())
            } finally {
                returnFromConstruction.countDown()
                creation.cancel()
                withTimeout(5_000) { creation.join() }
            }
        }
    }

    @Test
    fun engineDetachBeforeQueuedMainReturnDisposesUnregisteredManager() = runBlocking {
        Fixture().use { fixture ->
            val constructed = CountDownLatch(1)
            val returnFromConstruction = CountDownLatch(1)
            val mainBlocked = CountDownLatch(1)
            val releaseMain = CountDownLatch(1)
            val ioReturned = CountDownLatch(1)
            val manager = AtomicReference<Manager>()
            val creation = fixture.scope.launch {
                createAndRegisterDownloadManager(
                    createManager = {
                        Manager().also {
                            manager.set(it)
                            constructed.countDown()
                            check(returnFromConstruction.await(5, TimeUnit.SECONDS))
                        }
                    },
                    registerManager = fixture::register,
                    disposeManager = Manager::dispose,
                    ioDispatcher = fixture.ioDispatcher,
                )
            }
            try {
                assertTrue(constructed.await(5, TimeUnit.SECONDS))
                fixture.mainExecutor.execute {
                    mainBlocked.countDown()
                    releaseMain.await(5, TimeUnit.SECONDS)
                }
                assertTrue(mainBlocked.await(5, TimeUnit.SECONDS))
                // Engine detach is queued ahead of the IO -> Main continuation.
                fixture.mainExecutor.execute { fixture.detach() }
                returnFromConstruction.countDown()
                fixture.ioExecutor.execute { ioReturned.countDown() }
                assertTrue(ioReturned.await(5, TimeUnit.SECONDS))
                releaseMain.countDown()
                withTimeout(5_000) { creation.join() }

                assertTrue(creation.isCancelled)
                assertTrue(fixture.registered.isEmpty())
                assertEquals(1, manager.get().disposeCount.get())
            } finally {
                returnFromConstruction.countDown()
                releaseMain.countDown()
                creation.cancel()
                withTimeout(5_000) { creation.join() }
            }
        }
    }

    @Test
    fun successfulRegistrationTransfersDisposalToEngineDetach() = runBlocking {
        Fixture().use { fixture ->
            val creation = fixture.scope.async {
                createAndRegisterDownloadManager(
                    createManager = ::Manager,
                    registerManager = fixture::register,
                    disposeManager = Manager::dispose,
                    ioDispatcher = fixture.ioDispatcher,
                )
            }
            val manager = withTimeout(5_000) { creation.await() }
            assertSame(manager, fixture.registered.single())
            assertEquals(0, manager.disposeCount.get())
            withContext(fixture.mainDispatcher) { fixture.detach() }
            assertEquals(1, manager.disposeCount.get())
        }
    }

    @Test
    fun failedConstructorDoesNotPublishOrDisposeAnUncreatedManager() = runBlocking {
        Fixture().use { fixture ->
            val failure = IllegalStateException("construction failed")
            val disposeCount = AtomicInteger()
            val creation = fixture.scope.async {
                createAndRegisterDownloadManager<Manager, Manager>(
                    createManager = { throw failure },
                    registerManager = fixture::register,
                    disposeManager = { disposeCount.incrementAndGet() },
                    ioDispatcher = fixture.ioDispatcher,
                )
            }
            val actual = runCatching { withTimeout(5_000) { creation.await() } }.exceptionOrNull()
            assertEquals(failure.javaClass, actual?.javaClass)
            assertEquals(failure.message, actual?.message)
            assertTrue(fixture.registered.isEmpty())
            assertEquals(0, disposeCount.get())
        }
    }

    private class Manager {
        val disposeCount = AtomicInteger()

        fun dispose() {
            disposeCount.incrementAndGet()
        }
    }

    private class Fixture : AutoCloseable {
        val mainExecutor = Executors.newSingleThreadExecutor()
        val ioExecutor = Executors.newSingleThreadExecutor()
        val mainDispatcher = mainExecutor.asCoroutineDispatcher()
        val ioDispatcher = ioExecutor.asCoroutineDispatcher()
        val scope = CoroutineScope(SupervisorJob() + mainDispatcher)
        val registered = CopyOnWriteArrayList<Manager>()

        fun register(manager: Manager): Manager {
            registered.add(manager)
            return manager
        }

        fun detach() {
            registered.forEach(Manager::dispose)
            registered.clear()
            scope.cancel()
        }

        override fun close() {
            scope.cancel()
            mainDispatcher.close()
            ioDispatcher.close()
        }
    }
}
