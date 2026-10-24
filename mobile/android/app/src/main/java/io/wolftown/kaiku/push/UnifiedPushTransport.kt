package io.wolftown.kaiku.push

import android.content.Context
import dagger.hilt.android.qualifiers.ApplicationContext
import org.unifiedpush.android.connector.UnifiedPush
import javax.inject.Inject

/**
 * UnifiedPush: the distributor delivers the endpoint later to
 * [KaikuUnifiedPushService], which hands it to [PushRegistrar.onEndpointAsync].
 * So [requestEndpoint] only asks for a registration and never calls
 * `onEndpoint` itself. All connector calls are kept in this file. Endpoints
 * are never logged.
 */
class UnifiedPushTransport @Inject constructor(
    @ApplicationContext private val context: Context,
    private val preferences: PushPreferences,
) : PushTransport {

    override val kind: TransportKind = TransportKind.UNIFIED_PUSH

    override suspend fun requestEndpoint(onEndpoint: suspend (String) -> Unit) {
        val distributor = preferences.distributor
            ?: throw IllegalStateException("No UnifiedPush distributor selected")
        val saved = UnifiedPush.getSavedDistributor(context)
        // Switching distributors: drop the old registration so it stops delivering.
        if (saved != null && saved != distributor) UnifiedPush.unregister(context)
        UnifiedPush.saveDistributor(context, distributor)
        UnifiedPush.register(context)
    }

    override suspend fun unregister() {
        UnifiedPush.unregister(context)
    }
}
