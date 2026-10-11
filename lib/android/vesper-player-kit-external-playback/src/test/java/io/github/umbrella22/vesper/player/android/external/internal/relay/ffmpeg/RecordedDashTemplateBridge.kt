package io.github.umbrella22.vesper.player.android.external.internal.relay.ffmpeg

import org.json.JSONArray
import org.json.JSONObject

/** Uses the same golden requests and responses that the Rust bridge executes in its contract test. */
internal class RecordedDashTemplateBridge : (String) -> String {
    private val fixtures = JSONArray(
        javaClass.getResourceAsStream("/dash-template-bridge-contract.json")!!
            .bufferedReader().use { it.readText() },
    ).toJsonObjectList()

    val requests = mutableListOf<JSONObject>()

    override fun invoke(requestJson: String): String {
        val request = JSONObject(requestJson)
        requests += request
        val fixture = fixtures.firstOrNull { canonicalJson(it.getJSONObject("request")) == canonicalJson(request) }
            ?: throw AssertionError("No recorded DASH bridge request matches: $request")
        if (fixture.has("errorContains")) {
            throw IllegalArgumentException(fixture.getString("errorContains"))
        }
        val response = fixture.get("response")
        return if (response is String) JSONObject.quote(response) else response.toString()
    }
}

private fun canonicalJson(value: Any): String = when (value) {
    is JSONObject -> value.keys().asSequence().sorted().joinToString(prefix = "{", postfix = "}") {
        JSONObject.quote(it) + ":" + canonicalJson(value.get(it))
    }
    is JSONArray -> (0 until value.length()).joinToString(prefix = "[", postfix = "]") { canonicalJson(value.get(it)) }
    is String -> JSONObject.quote(value)
    else -> value.toString()
}

internal fun <T> withDashTemplateBridgeExecutor(executor: (String) -> String, block: () -> T): T {
    val previousApi = VesperRelayDashBridgeApiProvider
    val previousExecutor = VesperRelayDashBridgeExecutor
    VesperRelayDashBridgeApiProvider = JniVesperRelayDashBridgeApi
    VesperRelayDashBridgeExecutor = executor
    try {
        return block()
    } finally {
        VesperRelayDashBridgeApiProvider = previousApi
        VesperRelayDashBridgeExecutor = previousExecutor
    }
}
