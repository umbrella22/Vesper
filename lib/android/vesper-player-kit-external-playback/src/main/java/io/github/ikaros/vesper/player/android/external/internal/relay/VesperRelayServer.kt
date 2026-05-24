package io.github.ikaros.vesper.player.android.external.internal.relay

import android.content.Context
import io.github.ikaros.vesper.player.android.VesperPlayerSource
import java.net.InetAddress
import java.net.ServerSocket
import java.security.SecureRandom
import java.util.Base64
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicBoolean

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
) {
    private val appContext = context?.applicationContext
    private val random = SecureRandom()
    private val running = AtomicBoolean(false)
    private val entries = VesperRelayEntryStore(
        tokenTtlMillis = tokenTtlMillis,
        nowMillisProvider = nowMillisProvider,
        onInvalidate = formatAdapter::invalidate,
    )
    private val relaySource = VesperRelaySourceRelay(
        appContext = appContext,
        formatAdapter = formatAdapter,
        emitDiagnostic = ::emitDiagnostic,
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

    @Synchronized
    @JvmOverloads
    fun start(preferredBindAddress: InetAddress? = null) {
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
        val bindAddress = preferredBindAddress?.takeIf { it.isBindableLanAddress() }
            ?: bindAddressProvider()
            ?: appContext?.findWifiLanIpv4Address()
            ?: throw IllegalStateException("No Wi-Fi LAN address is available for relay.")
        val socket = ServerSocket(0, 50, bindAddress)
        serverSocket = socket
        boundAddress = bindAddress
        requestExecutor = Executors.newFixedThreadPool(maxRequestThreads.coerceAtLeast(1)) { runnable ->
            Thread(runnable, "vesper-relay-request").apply { isDaemon = true }
        }
        acceptExecutor = Executors.newSingleThreadExecutor { runnable ->
            Thread(runnable, "vesper-relay-accept").apply { isDaemon = true }
        }
        running.set(true)
        acceptExecutor?.execute {
            runRelayAcceptLoop(
                running = running,
                socket = socket,
                requestExecutorProvider = { requestExecutor },
                clientHandler = clientHandler,
            )
        }
    }

    @Synchronized
    fun stop() {
        running.set(false)
        entries.invalidateAll()
        runCatching { serverSocket?.close() }
        clientHandler.closeActiveClients()
        serverSocket = null
        boundAddress = null
        acceptExecutor?.shutdownNow()
        requestExecutor?.shutdownNow()
        acceptExecutor = null
        requestExecutor = null
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
        val socket = serverSocket ?: throw IllegalStateException("Relay server is not running.")
        val host = advertisedHost(preferredAddress)
            ?: throw IllegalStateException("No LAN address is available for relay.")
        val relayPath = source.relayPath(token, adaptation)
        entries.put(token, source, adaptation)
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
        return VesperRelayHandle(
            token = token,
            url = "http://$host:${socket.localPort}$relayPath",
        )
    }

    private fun advertisedHost(preferredAddress: InetAddress?): String? {
        val activeBind = boundAddress
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
private const val DEFAULT_MAX_ACTIVE_CLIENTS = 32
