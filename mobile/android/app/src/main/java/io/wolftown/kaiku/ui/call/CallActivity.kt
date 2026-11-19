package io.wolftown.kaiku.ui.call

import android.Manifest
import android.app.KeyguardManager
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.provider.Settings
import android.view.WindowManager
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.activity.viewModels
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.core.content.ContextCompat
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import dagger.hilt.android.AndroidEntryPoint
import io.wolftown.kaiku.data.call.CallController
import io.wolftown.kaiku.data.call.CallState
import io.wolftown.kaiku.data.call.signedInUserId
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.data.local.TokenStorage
import io.wolftown.kaiku.data.ws.RealtimeConnectionManager
import io.wolftown.kaiku.ui.shared.KaikuTheme
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.withTimeoutOrNull
import javax.inject.Inject

/**
 * The call screen as its own activity: it may show over the lock screen and
 * turn the screen on (API 27+ `setShowWhenLocked`/`setTurnScreenOn`, window
 * flags on API 26), opened by the incoming call's full-screen intent, its
 * Accept action, a tap on it or the ongoing-call notification, by the chat's
 * call button and by the app when a call rings while it is open. It shows
 * only the call (never chats) and closes when the call is over.
 *
 * Accept without the microphone permission first asks to unlock, then for
 * the permission; nothing is sent to the server before it is granted.
 */
@AndroidEntryPoint
class CallActivity : ComponentActivity() {

    @Inject lateinit var callController: CallController

    @Inject lateinit var authState: AuthState

    @Inject lateinit var tokenStorage: TokenStorage

    @Inject lateinit var realtimeConnectionManager: RealtimeConnectionManager

    private val viewModel: CallViewModel by viewModels()

    /** The DM of the ring an Accept is waiting for (permission, unlock). */
    private var acceptFor: String? = null

    private val micPermission = registerForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
        val channelId = acceptFor
        acceptFor = null
        viewModel.accept(granted, channelId)
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        showOverLockScreen()
        // Opened from a notification, the app may not have restored its session yet.
        authState.initialize(tokenStorage)
        if (savedInstanceState == null) handle(intent)

        setContent {
            KaikuTheme(darkTheme = true) {
                Surface(modifier = Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.background) {
                    val ui by viewModel.ui.collectAsStateWithLifecycle()
                    val micNeeded by viewModel.micNeeded.collectAsStateWithLifecycle()
                    CallScreen(
                        ui = ui,
                        micNeeded = micNeeded,
                        onAccept = { requestAccept(null) },
                        onDecline = viewModel::decline,
                        onHangUp = viewModel::hangUp,
                        onToggleMute = viewModel::toggleMute,
                        onToggleSpeaker = viewModel::toggleSpeaker,
                        onOpenSettings = ::openAppSettings,
                        onClose = {
                            viewModel.dismissEnded()
                            finish()
                        },
                    )
                    LaunchedEffect(Unit) { closeWhenOver() }
                }
            }
        }
    }

    override fun onStart() {
        super.onStart()
        // A socket that died while the phone slept is noticed now, not after the watchdog.
        realtimeConnectionManager.onAppForeground()
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        handle(intent)
    }

    private fun handle(intent: Intent?) {
        intent ?: return
        val parsed = CallIntents.parse(intent::getStringExtra)
        // A notification of another (or no) signed-in account is never acted on (C32).
        val ring = parsed.ring?.takeIf { it.userId == authState.signedInUserId() }
        ring?.let { callController.onRingNotification(it.channelId, it.callId, it.callerId, it.callerName) }
        if (parsed.action == CallIntents.ACTION_ACCEPT) {
            // Consumed: a recreation must not answer again.
            intent.putExtra(CallIntents.EXTRA_ACTION, CallIntents.ACTION_SHOW)
            if (ring != null || parsed.ring == null) requestAccept(ring?.channelId)
        }
    }

    /** Accept, asking to unlock and for the microphone first if it is not granted. */
    private fun requestAccept(channelId: String?) {
        val granted = ContextCompat.checkSelfPermission(this, Manifest.permission.RECORD_AUDIO) ==
            PackageManager.PERMISSION_GRANTED
        if (granted) {
            viewModel.accept(true, channelId)
            return
        }
        acceptFor = channelId
        val keyguard = getSystemService(KeyguardManager::class.java)
        if (keyguard != null && keyguard.isKeyguardLocked) {
            // The permission dialog needs an unlocked phone.
            keyguard.requestDismissKeyguard(this, object : KeyguardManager.KeyguardDismissCallback() {
                override fun onDismissSucceeded() {
                    micPermission.launch(Manifest.permission.RECORD_AUDIO)
                }

                override fun onDismissCancelled() {
                    viewModel.accept(false, channelId)
                }

                override fun onDismissError() {
                    viewModel.accept(false, channelId)
                }
            })
        } else {
            micPermission.launch(Manifest.permission.RECORD_AUDIO)
        }
    }

    /**
     * Finishes once the call is over (idle after its end was shown). A screen
     * opened for a ring that is already over (a stale notification) closes
     * after a moment.
     */
    private suspend fun closeWhenOver() {
        val seen = withTimeoutOrNull(OPEN_GRACE_MS) {
            callController.state.first { it !is CallState.Idle }
        }
        if (seen == null) {
            finish()
            return
        }
        callController.state.first { it is CallState.Idle }
        delay(CLOSE_DELAY_MS)
        if (callController.state.value is CallState.Idle) finish()
    }

    private fun openAppSettings() {
        startActivity(
            Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.fromParts("package", packageName, null))
                .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        )
    }

    private fun showOverLockScreen() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O_MR1) {
            setShowWhenLocked(true)
            setTurnScreenOn(true)
        } else {
            @Suppress("DEPRECATION")
            window.addFlags(WindowManager.LayoutParams.FLAG_SHOW_WHEN_LOCKED or WindowManager.LayoutParams.FLAG_TURN_SCREEN_ON)
        }
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
    }

    private companion object {
        /** How long a screen opened without a call waits for one (the ring is applied asynchronously). */
        const val OPEN_GRACE_MS = 3_000L

        /** Pause before closing, so a call that starts again right away keeps the screen. */
        const val CLOSE_DELAY_MS = 300L
    }
}
