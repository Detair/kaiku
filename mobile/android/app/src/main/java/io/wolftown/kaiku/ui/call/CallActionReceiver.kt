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

/**
 * The broadcasts of an incoming-call notification: its Decline action, and
 * the notification going away by itself. Both name the account, the
 * conversation and the call the notification was for and go to the call
 * owner like every other input ([CallIntents.inputOf]); a notification of
 * another account, or of a call that is no longer the one shown or ringing,
 * changes nothing.
 */
@AndroidEntryPoint
class CallActionReceiver : BroadcastReceiver() {

    @Inject lateinit var callController: CallController

    @Inject lateinit var tokenStorage: TokenStorage

    @Inject lateinit var authState: AuthState

    override fun onReceive(context: Context, intent: Intent) {
        val ring = CallIntents.parse(intent::getStringExtra).ring ?: return
        val input = CallIntents.inputOf(intent.action, ring, System.currentTimeMillis()) ?: return
        // The app may run just for this broadcast. A decline is a request to the server and
        // needs the session restored; the declined call is then also recorded for this account.
        if (intent.action == CallIntents.ACTION_DECLINE) authState.initialize(tokenStorage)
        // Keep the process alive until the owner has handled it (and the server was told).
        val pending = goAsync()
        CoroutineScope(Dispatchers.IO).launch {
            try {
                withTimeoutOrNull(ACTION_TIMEOUT_MS) { callController.submitAndAwait(input) }
            } finally {
                pending.finish()
            }
        }
    }

    private companion object {
        /** Below the 10 s a broadcast may take. */
        const val ACTION_TIMEOUT_MS = 8_000L
    }
}
