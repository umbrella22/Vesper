package io.github.umbrella22.vesper.player.android.external.internal.relay.ffmpeg

import android.system.ErrnoException
import android.system.Os
import android.system.OsConstants
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import java.io.ByteArrayOutputStream
import java.io.File
import java.io.FileDescriptor
import java.io.FileOutputStream
import java.io.IOException
import java.net.SocketTimeoutException
import java.util.concurrent.CountDownLatch
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors
import java.util.concurrent.Future
import java.util.concurrent.TimeUnit
import java.util.concurrent.TimeoutException
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicReference
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertSame
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class VesperRelayHostPreparedDashFifoInstrumentationTest {
    @Test
    fun tinyTrackWaitsForReaderThenDeliversEveryByteAndEof() = withFifo { pipe ->
        val bytes = byteArrayOf(0, 1, 2, 127, -128, -1)
        withExecutor { executor ->
            val started = CountDownLatch(1)
            val writer = executor.submit {
                started.countDown()
                openHostPreparedDashFifoWriter(pipe.path, AtomicBoolean(false)).use { output ->
                    output.write(bytes)
                }
            }
            assertTrue(started.await(2, TimeUnit.SECONDS))
            assertPending(writer)

            withReader(pipe) { reader ->
                writer.get(3, TimeUnit.SECONDS)
                // A leaked raw writer or a self-reader would prevent this EOF.
                assertArrayEquals(bytes, readThroughEof(reader))
            }
        }
    }

    @Test
    fun ownedWriterUsesBlockingWriteOnlyFdAndClosesIdempotently() = withFifo { pipe ->
        withReader(pipe) { reader ->
            val bytes = byteArrayOf(3, 4, 5)
            val output = openHostPreparedDashFifoWriter(pipe.path, AtomicBoolean(false))
            val descriptor = (output as FileOutputStream).fd
            try {
                val flags = Os.fcntlInt(descriptor, OsConstants.F_GETFL, 0)
                assertEquals(OsConstants.O_WRONLY, flags and OsConstants.O_ACCMODE)
                assertEquals(0, flags and OsConstants.O_NONBLOCK)
                output.write(bytes)
                output.close()
                output.close()
                assertFalse(descriptor.valid())
                assertArrayEquals(bytes, readThroughEof(reader))
            } finally {
                output.close()
            }
        }
    }

    @Test
    fun noReaderTimesOutWithinTheConfiguredEstablishmentBudget() = withFifo { pipe ->
        val started = System.nanoTime()
        assertThrows(SocketTimeoutException::class.java) {
            openHostPreparedDashFifoWriter(pipe.path, AtomicBoolean(false), readerTimeoutMillis = 150)
        }
        val elapsedMillis = TimeUnit.NANOSECONDS.toMillis(System.nanoTime() - started)
        assertTrue("Timeout must honor its budget: $elapsedMillis ms", elapsedMillis >= 140)
        assertTrue("No-reader open must stay bounded: $elapsedMillis ms", elapsedMillis < 2_000)

        withReader(pipe) { reader ->
            openHostPreparedDashFifoWriter(pipe.path, AtomicBoolean(false)).close()
            assertArrayEquals(ByteArray(0), readThroughEof(reader))
        }
    }

    @Test
    fun cancellationBeforeOpenSkipsDescriptorAndStreamCreation() = withFifo { pipe ->
        val factoryCalled = AtomicBoolean(false)
        assertThrows(HostInputCancelledException::class.java) {
            openHostPreparedDashFifoWriter(
                pipe.path,
                AtomicBoolean(true),
                createOutputStream = {
                    factoryCalled.set(true)
                    throw AssertionError("Cancelled writer must not create a stream")
                },
            )
        }
        assertFalse(factoryCalled.get())
    }

    @Test
    fun cancellationDuringRendezvousReturnsPromptly() = withFifo { pipe ->
        val cancellation = AtomicBoolean(false)
        withExecutor { executor ->
            val started = CountDownLatch(1)
            val writer = executor.submit<Throwable> {
                started.countDown()
                assertThrows(HostInputCancelledException::class.java) {
                    openHostPreparedDashFifoWriter(pipe.path, cancellation).close()
                }
            }
            assertTrue(started.await(2, TimeUnit.SECONDS))
            assertPending(writer)
            val cancelStarted = System.nanoTime()
            cancellation.set(true)
            assertTrue(writer.get(1, TimeUnit.SECONDS) is HostInputCancelledException)
            assertTrue(TimeUnit.NANOSECONDS.toMillis(System.nanoTime() - cancelStarted) < 1_000)
        }
    }

    @Test
    fun interruptCancelsRendezvousAndPreservesTheInterruptFlag() = withFifo { pipe ->
        val started = CountDownLatch(1)
        val finished = CountDownLatch(1)
        val failure = AtomicReference<Throwable>()
        val interrupted = AtomicBoolean(false)
        val writer = Thread({
            try {
                started.countDown()
                openHostPreparedDashFifoWriter(pipe.path, AtomicBoolean(false)).close()
            } catch (error: Throwable) {
                failure.set(error)
                interrupted.set(Thread.currentThread().isInterrupted)
            } finally {
                finished.countDown()
            }
        }, "vesper-fifo-interrupt-test").apply { isDaemon = true }
        writer.start()
        try {
            assertTrue(started.await(2, TimeUnit.SECONDS))
            assertFalse(finished.await(100, TimeUnit.MILLISECONDS))
            writer.interrupt()
            assertTrue(finished.await(1, TimeUnit.SECONDS))
            assertTrue(failure.get() is HostInputCancelledException)
            assertTrue(interrupted.get())
        } finally {
            writer.interrupt()
            writer.join(2_000)
            assertFalse("Writer thread must terminate", writer.isAlive)
        }
    }

    @Test
    fun missingPathPropagatesOpenErrorWithoutRetrying() = withFifo { pipe ->
        val started = System.nanoTime()
        val error = assertThrows(ErrnoException::class.java) {
            openHostPreparedDashFifoWriter(pipe.path + ".missing", AtomicBoolean(false)).close()
        }
        assertEquals(OsConstants.ENOENT, error.errno)
        assertTrue(TimeUnit.NANOSECONDS.toMillis(System.nanoTime() - started) < 2_000)
    }

    @Test
    fun constructorFailureClosesOwnedDuplicateAndRawWriter() = withFifo { pipe ->
        withReader(pipe) { reader ->
            val failure = IOException("stream-constructor-test")
            val duplicate = AtomicReference<FileDescriptor>()
            val error = assertThrows(IOException::class.java) {
                openHostPreparedDashFifoWriter(
                    pipe.path,
                    AtomicBoolean(false),
                    createOutputStream = { descriptor ->
                        duplicate.set(descriptor.fileDescriptor)
                        throw failure
                    },
                )
            }
            assertSame(failure, error)
            assertFalse(duplicate.get().valid())
            assertArrayEquals(ByteArray(0), readThroughEof(reader))
        }
    }

    @Test
    fun backpressureResumesWhenReaderDrainsAndEndsAtEof() = withFifo { pipe ->
        withReader(pipe) { reader ->
            val bytes = ByteArray(2 * 1024 * 1024) { (it % 251).toByte() }
            withExecutor { executor ->
                val output = openHostPreparedDashFifoWriter(pipe.path, AtomicBoolean(false))
                var writer: Future<*>? = null
                try {
                    val started = CountDownLatch(1)
                    writer = executor.submit {
                        output.use {
                            started.countDown()
                            it.write(bytes)
                        }
                    }
                    assertTrue(started.await(2, TimeUnit.SECONDS))
                    assertPending(writer)
                    assertArrayEquals(bytes, readThroughEof(reader))
                    writer.get(3, TimeUnit.SECONDS)
                } finally {
                    output.close()
                    if (writer?.isDone == false) {
                        Os.close(reader)
                    }
                }
            }
        }
    }

    @Test
    fun closingOwnedWriterCancelsABackpressuredWrite() = withFifo { pipe ->
        withReader(pipe) { reader ->
            withExecutor { executor ->
                val output = openHostPreparedDashFifoWriter(pipe.path, AtomicBoolean(false))
                var writer: Future<*>? = null
                try {
                    val started = CountDownLatch(1)
                    writer = executor.submit<Throwable> {
                        started.countDown()
                        assertThrows(IOException::class.java) { output.write(ByteArray(2 * 1024 * 1024)) }
                    }
                    assertTrue(started.await(2, TimeUnit.SECONDS))
                    assertPending(writer)
                    // Keep the reader open and undrained until cancellation is proven.
                    output.close()
                    assertTrue(writer.get(3, TimeUnit.SECONDS) is IOException)
                    readThroughEof(reader)
                } finally {
                    output.close()
                    // Release a stalled kernel write even when the assertion fails.
                    if (writer?.isDone == false) {
                        Os.close(reader)
                    }
                }
            }
        }
    }

    private fun assertPending(writer: Future<*>) {
        assertThrows(TimeoutException::class.java) { writer.get(100, TimeUnit.MILLISECONDS) }
    }

    private fun withFifo(block: (File) -> Unit) {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val directory = File.createTempFile("vesper-fifo-test-", "", context.cacheDir)
        check(directory.delete() && directory.mkdir())
        val pipe = File(directory, "track.fifo")
        try {
            Os.mkfifo(pipe.path, OsConstants.S_IRUSR or OsConstants.S_IWUSR)
            block(pipe)
        } finally {
            directory.deleteRecursively()
        }
    }

    private fun withReader(pipe: File, block: (FileDescriptor) -> Unit) {
        val reader = Os.open(pipe.path, OsConstants.O_RDONLY or OsConstants.O_NONBLOCK, 0)
        try {
            block(reader)
        } finally {
            if (reader.valid()) {
                Os.close(reader)
            }
        }
    }

    private fun withExecutor(block: (ExecutorService) -> Unit) {
        val executor = Executors.newSingleThreadExecutor { task ->
            Thread(task, "vesper-fifo-writer-test").apply { isDaemon = true }
        }
        try {
            block(executor)
        } finally {
            executor.shutdownNow()
            assertTrue("Writer executor must terminate", executor.awaitTermination(3, TimeUnit.SECONDS))
        }
    }

    private fun readThroughEof(reader: FileDescriptor): ByteArray {
        val deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(3)
        val bytes = ByteArrayOutputStream()
        val buffer = ByteArray(8192)
        while (System.nanoTime() < deadline) {
            val count = try {
                Os.read(reader, buffer, 0, buffer.size)
            } catch (error: ErrnoException) {
                if (error.errno != OsConstants.EAGAIN) {
                    throw error
                }
                Thread.sleep(5)
                continue
            }
            if (count == 0) {
                return bytes.toByteArray()
            }
            bytes.write(buffer, 0, count)
        }
        throw AssertionError("FIFO reader did not receive normal EOF within 3 seconds")
    }
}
