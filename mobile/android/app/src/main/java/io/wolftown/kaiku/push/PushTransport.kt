package io.wolftown.kaiku.push

/** A push transport and its provider name on the wire (C19 `provider`). */
enum class TransportKind(val wire: String) {
    FCM("fcm"),
    UNIFIED_PUSH("unifiedpush"),
}

/** The UnifiedPush transport has no distributor to register with (none chosen, saved or unambiguous). */
class NoDistributorException : Exception("No UnifiedPush distributor available")

/**
 * Obtains the endpoint the server pushes to. FCM hands over its registration
 * token right away; UnifiedPush (Task 8) delivers its endpoint later through
 * its receiver. Implementations never log the endpoint.
 */
interface PushTransport {
    val kind: TransportKind

    /** Requests an endpoint and passes it to [onEndpoint] (possibly later, for UnifiedPush). */
    suspend fun requestEndpoint(onEndpoint: suspend (String) -> Unit)

    /** Gives up the endpoint, so a server copy left behind stops working. */
    suspend fun unregister()
}
