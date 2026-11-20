package io.wolftown.kaiku.ui.call

import android.Manifest
import android.app.KeyguardManager
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.PowerManager
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
import io.wolftown.kaiku.data.call.CallInput
import io.wolftown.kaiku.data.call.CallRef
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

    // The ring an Accept is waiting for (permission, unlock) is kept by the view model, with
    // its account, conversation and call: the answer is for that call only.
    private val micPermission = registerForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
        viewModel.permissionResult(granted)
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
                    LaunchedEffect(ui) { if (ui.phase != CallPhase.None) apply(CallScreenPolicy.of(ui)) }
                    CallScreen(
                        ui = ui,
                        micNeeded = micNeeded,
                        onAccept = { requestAccept(ui.call) },
                        onDecline = { viewModel.decline(ui.call) },
                        onHangUp = { viewModel.hangUp(ui.call) },
                        onToggleMute = { viewModel.setMuted(ui.call, !ui.muted) },
                        onToggleSpeaker = viewModel::toggleSpeaker,
                        onOpenSettings = ::openAppSettings,
                        onClose = {
                            viewModel.dismissEnded(ui.call)
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
        isStarted = true
        applied?.let { if (it.proximityScreenOff) proximity.acquire() }
        // A socket that died while the phone slept is noticed now, not after the watchdog.
        realtimeConnectionManager.onAppForeground()
    }

    override fun onStop() {
        // Not visible: nothing to turn off at the ear.
        isStarted = false
        proximity.release()
        super.onStop()
    }

    override fun onDestroy() {
        proximity.release()
        super.onDestroy()
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        handle(intent)
    }

    private fun handle(intent: Intent?) {
        intent ?: return
        val parsed = CallIntents.parse(intent::getStringExtra)
        // A notification of another (or no) signed-in account is never acted on (the call
        // owner drops it as well).
        val ring = parsed.ring?.takeIf { it.userId == authState.signedInUserId() }
        ring?.let {
            callController.submit(CallInput.RingNotification(it.ref(), it.callerId, it.callerName, it.receivedAtMs))
        }
        if (parsed.action == CallIntents.ACTION_ACCEPT) {
            // Consumed: a recreation must not answer again.
            intent.putExtra(CallIntents.EXTRA_ACTION, CallIntents.ACTION_SHOW)
            // The notification's own call; without one in the intent, the call shown now.
            requestAccept(ring?.ref() ?: callController.shown().ref().takeIf { parsed.ring == null })
        }
    }

    /** Accept of [call], asking to unlock and for the microphone first if it is not granted. */
    private fun requestAccept(call: CallRef?) {
        call ?: return
        val granted = ContextCompat.checkSelfPermission(this, Manifest.permission.RECORD_AUDIO) ==
            PackageManager.PERMISSION_GRANTED
        if (granted) {
            viewModel.accept(call, true)
            return
        }
        viewModel.acceptNeedsPermission(call)
        val keyguard = getSystemService(KeyguardManager::class.java)
        if (keyguard != null && keyguard.isKeyguardLocked) {
            // The permission dialog needs an unlocked phone.
            keyguard.requestDismissKeyguard(this, object : KeyguardManager.KeyguardDismissCallback() {
                override fun onDismissSucceeded() {
                    micPermission.launch(Manifest.permission.RECORD_AUDIO)
                }

                override fun onDismissCancelled() {
                    viewModel.permissionResult(false)
                }

                override fun onDismissError() {
                    viewModel.permissionResult(false)
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

    /** Until the call is known: as for a ring (a full-screen intent must show over the lock screen). */
    private fun showOverLockScreen() = apply(CallScreenFlags(keepScreenOn = true, turnScreenOn = true, showWhenLocked = true, proximityScreenOff = false))

    private var applied: CallScreenFlags? = null

    /** Applies [flags] to the window and the proximity wake lock (only what changed). */
    private fun apply(flags: CallScreenFlags) {
        if (flags == applied) return
        applied = flags
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O_MR1) {
            setShowWhenLocked(flags.showWhenLocked)
            setTurnScreenOn(flags.turnScreenOn)
        } else {
            @Suppress("DEPRECATION")
            setWindowFlag(WindowManager.LayoutParams.FLAG_SHOW_WHEN_LOCKED, flags.showWhenLocked)
            @Suppress("DEPRECATION")
            setWindowFlag(WindowManager.LayoutParams.FLAG_TURN_SCREEN_ON, flags.turnScreenOn)
        }
        setWindowFlag(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON, flags.keepScreenOn)
        if (flags.proximityScreenOff && isStarted) proximity.acquire() else proximity.release()
    }

    private fun setWindowFlag(flag: Int, on: Boolean) {
        if (on) window.addFlags(flag) else window.clearFlags(flag)
    }

    private var isStarted = false

    /** The display off while the phone is at the ear (only while this screen is visible). */
    private val proximity by lazy { ProximityLock(getSystemService(PowerManager::class.java)) }

    private companion object {
        /** How long a screen opened without a call waits for one (the ring is applied asynchronously). */
        const val OPEN_GRACE_MS = 3_000L

        /** Pause before closing, so a call that starts again right away keeps the screen. */
        const val CLOSE_DELAY_MS = 300L
    }
}
