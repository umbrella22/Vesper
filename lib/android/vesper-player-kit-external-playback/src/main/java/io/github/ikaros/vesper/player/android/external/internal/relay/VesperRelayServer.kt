package io.github.ikaros.vesper.player.android.external.internal.relay

import android.content.Context
import io.github.ikaros.vesper.player.android.VesperPlayerSource
import java.net.InetAddress
import java.net.ServerSocket
import java.security.SecureRandom
import java.util.Base64
import java.util.concurrent.ArrayBlockingQueue
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors
import java.util.concurrent.ThreadPoolExecutor
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicLong
import java.util.concurrent.locks.ReentrantLock
import kotlin.concurrent.withLock

data class VesperRelayHandle(
    val token: String,
    val url: String,
)

class VesperRelayRegistrationException(
    val status: Int,
    val diagnostic: VesperRelayDiagnostic,
) : Exception(diagnostic.message)

class VesperRelayServer @JvmOverloads constructor(
    context: Context? = null,
    private val advertisedAddressProvider: () -> InetAddress? = ::findLanIpv4Address,
    private val bindAddressProvider: () -> InetAddress? = { context?.findWifiLanIpv4Address() },
    private val tokenTtlMillis: Long? = DEFAULT_TOKEN_TTL_MILLIS,
    private val nowMillisProvider: () -> Long = System::currentTimeMillis,
    private val formatAdapter: VesperRelayFormatAdapter = VesperUnavailableRelayFormatAdapter(),
    private val diagnosticListener: (VesperRelayDiagnostic) -> Unit = {},
    private val maxRequestThreads: Int = DEFAULT_MAX_REQUEST_THREADS,
    private val maxActiveClients: Int = DEFAULT_MAX_ACTIVE_CLIENTS,
    private val allowPrivateRemoteSources: Boolean = false,
) {
    private val appContext = context?.applicationContext
    private val random = SecureRandom()
    private val running = AtomicBoolean(false)
    private val lifecycleEpoch = AtomicLong(0L)
    private val entries = VesperRelayEntryStore(
        tokenTtlMillis = tokenTtlMillis,
        nowMillisProvider = nowMillisProvider,
        onInvalidate = formatAdapter::invalidate,
    )
    private val relaySource = VesperRelaySourceRelay(
        appContext = appContext,
        formatAdapter = formatAdapter,
        emitDiagnostic = ::emitDiagnostic,
        allowPrivateRemoteSources = allowPrivateRemoteSources,
    )
    private val clientHandler = VesperRelayClientHandler(
        running = running,
        maxActiveClients = maxActiveClients,
        entryForToken = entries::entryForToken,
        relaySource = relaySource,
    )
    @Volatile
    private var serverSocket: ServerSocket? = null
    @Volatile
    private var acceptExecutor: ExecutorService? = null
    @Volatile
    private var requestExecutor: ExecutorService? = null
    @Volatile
    private var boundAddress: InetAddress? = null
    private val stateLock = ReentrantLock()

    /**
     * Snapshot of resources owned by a running relay server. Captured under
     * [stateLock] and torn down outside the lock so blocking work (socket
     * close, executor shutdown, client teardown) never holds the monitor.
     */
    private class RunningState(
        val socket: ServerSocket,
        val boundAddress: InetAddress,
        val acceptExecutor: ExecutorService,
        val requestExecutor: ExecutorService,
    )

    /**
     * Brings the relay server up on [preferredBindAddress] (or an auto-detected
     * Wi-Fi LAN address).
     *
     * The monitor is held only to read/swap the volatile running-state fields.
     * LAN address enumeration, socket bind, and executor construction happen
     * *outside* [stateLock] because they are blocking network operations. A
     * concurrent [stop] during setup is detected on re-lock and the freshly
     * built resources are torn down instead of being published.
     */
    @JvmOverloads
    fun start(preferredBindAddress: InetAddress? = null) {
        // Fast path: already running on an address that satisfies the request.
        // Touch only @Volatile fields here; no lock, no blocking.
        if (running.get()) {
            val preferredAddress = preferredBindAddress?.takeIf { it.isBindableLanAddress() }
            val currentAddress = boundAddress
            if (preferredAddress == null ||
                currentAddress?.isAnyLocalAddress == true ||
                currentAddress?.hasSameHostAddress(preferredAddress) == true ||
                entries.isNotEmpty
            ) {
                return
            }
            stop()
        }

        val startEpoch = lifecycleEpoch.get()

        // Bind + executor construction are blocking network/OS operations;
        // perform them outside the monitor.
        val bindAddress = preferredBindAddress?.takeIf { it.isBindableLanAddress() }
            ?: bindAddressProvider()
            ?: appContext?.findWifiLanIpv4Address()
            ?: throw IllegalStateException("No Wi-Fi LAN address is available for relay.")
        val socket = ServerSocket(0, 50, bindAddress)
        val requestExecutor =
            ThreadPoolExecutor(
                maxRequestThreads.coerceAtLeast(1),
                maxRequestThreads.coerceAtLeast(1),
                0L,
                TimeUnit.MILLISECONDS,
                ArrayBlockingQueue(DEFAULT_MAX_QUEUED_REQUESTS),
                { runnable ->
                    Thread(runnable, "vesper-relay-request").apply { isDaemon = true }
                },
                ThreadPoolExecutor.AbortPolicy(),
            )
        val acceptExecutor = Executors.newSingleThreadExecutor { runnable ->
            Thread(runnable, "vesper-relay-accept").apply { isDaemon = true }
        }

        // Publish under the lock. If another start() already won the race to
        // flip running from false -> true while we were building resources
        // off-lock, tear down what we just built without touching running
        // (the winner owns it now).
        val lostTheRace = stateLock.withLock {
            if (lifecycleEpoch.get() != startEpoch || !running.compareAndSet(false, true)) {
                true
            } else {
                serverSocket = socket
                boundAddress = bindAddress
                this.acceptExecutor = acceptExecutor
                this.requestExecutor = requestExecutor
                acceptExecutor.execute {
                    runRelayAcceptLoop(
                        running = running,
                        socket = socket,
                        requestExecutorProvider = { this.requestExecutor },
                        clientHandler = clientHandler,
                    )
                }
                false
            }
        }
        if (lostTheRace) {
            runCatching { socket.close() }
            acceptExecutor.shutdownNow()
            requestExecutor.shutdownNow()
        }
    }

    /**
     * Tears the relay server down. The monitor is held only long enough to
     * detach the running-state fields; socket close, executor shutdown, and
     * active-client teardown all run *outside* [stateLock].
     */
    fun stop() {
        val state = stateLock.withLock {
            lifecycleEpoch.incrementAndGet()
            running.set(false)
            entries.invalidateAll()
            val captured = RunningState(
                socket = serverSocket ?: return@withLock null,
                boundAddress = boundAddress ?: return@withLock null,
                acceptExecutor = acceptExecutor ?: return@withLock null,
                requestExecutor = requestExecutor ?: return@withLock null,
            )
            serverSocket = null
            boundAddress = null
            this.acceptExecutor = null
            this.requestExecutor = null
            captured
        } ?: run {
            // Nothing was running; still ensure clients/entries are cleared so
            // callers see a clean quiescent state. closeActiveClients is the
            // only potentially blocking call here and runs without the lock.
            clientHandler.closeActiveClients()
            return
        }
        // Blocking teardown outside the monitor.
        runCatching { state.socket.close() }
        clientHandler.closeActiveClients()
        state.acceptExecutor.shutdownNow()
        state.requestExecutor.shutdownNow()
    }

    @JvmOverloads
    fun register(
        source: VesperPlayerSource,
        adaptation: VesperRelayFormatAdaptationRegistration? = null,
        preferredAddress: InetAddress? = null,
    ): VesperRelayHandle {
        pruneExpiredEntries()
        val token = nextToken()
        adaptation?.let { registration ->
            val validationRequest = source.toFormatAdaptationRequest(
                token = token,
                adaptation = registration,
                resourcePath = "",
                headOnly = false,
                range = null,
                requestHeaders = emptyMap(),
            )
            formatAdapter.validate(validationRequest)?.let { failure ->
                val diagnostic = failure.diagnostic.withHttpStatus(failure.status)
                emitDiagnostic(diagnostic)
                throw VesperRelayRegistrationException(failure.status, diagnostic)
            }
        }
        start(preferredAddress)
        val registerEpoch = lifecycleEpoch.get()
        val socket = serverSocket ?: throw IllegalStateException("Relay server is not running.")
        val activeBind = boundAddress
        val host = advertisedHost(preferredAddress, activeBind)
            ?: throw IllegalStateException("No LAN address is available for relay.")
        val relayPath = source.relayPath(token, adaptation)
        stateLock.withLock {
            if (
                lifecycleEpoch.get() != registerEpoch ||
                    !running.get() ||
                    serverSocket !== socket
            ) {
                throw IllegalStateException("Relay server stopped during registration.")
            }
            entries.put(token, source, adaptation)
        }
        try {
            adaptation?.let { registration ->
                val prewarmRequest = source.toFormatAdaptationRequest(
                    token = token,
                    adaptation = registration,
                    resourcePath = relayPath.substringAfterLast('/', missingDelimiterValue = ""),
                    headOnly = false,
                    range = null,
                    requestHeaders = emptyMap(),
                )
                formatAdapter.prewarm(prewarmRequest)?.let { failure ->
                    val diagnostic = failure.diagnostic.withHttpStatus(failure.status)
                    emitDiagnostic(diagnostic)
                    throw VesperRelayRegistrationException(failure.status, diagnostic)
                }
            }
        } catch (error: VesperRelayRegistrationException) {
            entries.remove(token)
            formatAdapter.invalidate(token)
            throw error
        } catch (error: RuntimeException) {
            entries.remove(token)
            formatAdapter.invalidate(token)
            throw error
        }
        val stillRegistered = stateLock.withLock {
            lifecycleEpoch.get() == registerEpoch &&
                running.get() &&
                serverSocket === socket
        }
        if (!stillRegistered) {
            entries.remove(token)
            formatAdapter.invalidate(token)
            throw IllegalStateException("Relay server stopped during registration.")
        }
        return VesperRelayHandle(
            token = token,
            url = "http://$host:${socket.localPort}$relayPath",
        )
    }

    private fun advertisedHost(
        preferredAddress: InetAddress?,
        activeBind: InetAddress? = boundAddress,
    ): String? {
        val preferred = preferredAddress?.takeIf { it.isAdvertisableLanAddress() }
        val address = when {
            preferred != null &&
                (activeBind == null ||
                    activeBind.isAnyLocalAddress ||
                    activeBind.hasSameHostAddress(preferred)) -> preferred
            activeBind != null && !activeBind.isAnyLocalAddress -> activeBind
            else -> appContext?.findWifiLanIpv4Address() ?: advertisedAddressProvider()
        }
        return address?.toRelayHost()
    }

    fun invalidate(token: String) {
        entries.invalidate(token)
    }

    fun invalidateAll() {
        entries.invalidateAll()
    }

    private fun pruneExpiredEntries() {
        entries.pruneExpiredEntries()
    }

    private fun emitDiagnostic(diagnostic: VesperRelayDiagnostic) {
        diagnosticListener(diagnostic)
    }

    private fun nextToken(): String {
        val bytes = ByteArray(24)
        random.nextBytes(bytes)
        return Base64.getUrlEncoder().withoutPadding().encodeToString(bytes)
    }
}

private const val DEFAULT_TOKEN_TTL_MILLIS = 30 * 60 * 1000L
private const val DEFAULT_MAX_REQUEST_THREADS = 16
private const val DEFAULT_MAX_QUEUED_REQUESTS = 64
private const val DEFAULT_MAX_ACTIVE_CLIENTS = 32
