package io.wolftown.kaiku.ui.call

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import dagger.hilt.android.AndroidEntryPoint
import io.wolftown.kaiku.data.call.CallController
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.data.local.TokenStorage
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeoutOrNull
import javax.inject.Inject

/** The Decline action of an incoming-call notification. */
@AndroidEntryPoint
class CallActionReceiver : BroadcastReceiver() {

    @Inject lateinit var callController: CallController

    @Inject lateinit var tokenStorage: TokenStorage

    @Inject lateinit var authState: AuthState

    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action != CallIntents.ACTION_DECLINE) return
        val ring = CallIntents.parse(intent::getStringExtra).ring ?: return
        // The app may run just for this broadcast: restore the session, so the declined call is
        // recorded for this account and a late ring of it does not ring again.
        authState.initialize(tokenStorage)
        // A notification of another account (or of none) is never acted on.
        if (ring.userId != tokenStorage.getUserId()) return
        // The app may run just for this broadcast: keep it alive until the server was told.
        val pending = goAsync()
        CoroutineScope(Dispatchers.IO).launch {
            try {
                withTimeoutOrNull(DECLINE_TIMEOUT_MS) {
                    callController.declineFromNotification(ring.channelId, ring.callId)
                }
            } finally {
                pending.finish()
            }
        }
    }

    private companion object {
        /** Below the 10 s a broadcast may take. */
        const val DECLINE_TIMEOUT_MS = 8_000L
    }
}
