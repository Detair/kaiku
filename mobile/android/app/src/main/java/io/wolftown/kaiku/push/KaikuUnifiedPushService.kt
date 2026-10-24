package io.wolftown.kaiku.push

import dagger.hilt.android.AndroidEntryPoint
import org.unifiedpush.android.connector.FailedReason
import org.unifiedpush.android.connector.PushService
import org.unifiedpush.android.connector.data.PushEndpoint
import org.unifiedpush.android.connector.data.PushMessage
import java.util.logging.Logger
import javax.inject.Inject

/**
 * Thin UnifiedPush receiver: parses the message and hands it to
 * [WakeScheduler] (the FCM pipeline); the fetch runs in the worker, never
 * here. The body is plaintext JSON (C20), so `decrypted` is ignored. Never
 * logs endpoints or message bodies.
 */
@AndroidEntryPoint
class KaikuUnifiedPushService : PushService() {

    @Inject
    lateinit var registrar: PushRegistrar

    @Inject
    lateinit var wakeScheduler: WakeScheduler

    override fun onNewEndpoint(endpoint: PushEndpoint, instance: String) {
        registrar.onEndpointAsync(TransportKind.UNIFIED_PUSH, endpoint.url)
    }

    override fun onMessage(message: PushMessage, instance: String) {
        handleUnifiedPushMessage(message.content, wakeScheduler::onWake)
    }

    override fun onRegistrationFailed(reason: FailedReason, instance: String) {
        logger.info("unifiedpush registration failed: reason=${reason.name}")
        registrar.onTransportRegistrationFailed()
    }

    override fun onUnregistered(instance: String) {
        logger.info("unifiedpush unregistered")
        registrar.onTransportUnregistered()
    }

    private companion object {
        val logger: Logger = Logger.getLogger("KaikuUnifiedPushService")
    }
}

/** Parses a UnifiedPush body (C20 JSON) and passes a valid wake signal on; returns whether one was found. */
internal fun handleUnifiedPushMessage(content: ByteArray, onWake: (WakeSignal) -> Unit): Boolean {
    val signal = WakeSignalParser.fromUnifiedPushBody(content) ?: return false
    onWake(signal)
    return true
}
