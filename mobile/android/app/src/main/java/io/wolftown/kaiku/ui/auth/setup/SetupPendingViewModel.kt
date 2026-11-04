package io.wolftown.kaiku.ui.auth.setup

import androidx.annotation.StringRes
import androidx.fragment.app.FragmentActivity
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import dagger.hilt.android.lifecycle.HiltViewModel
import io.wolftown.kaiku.R
import io.wolftown.kaiku.data.api.ApiException
import io.wolftown.kaiku.data.api.EnrollmentApi
import io.wolftown.kaiku.data.api.LongPoll
import io.wolftown.kaiku.data.auth.DeviceSignIn
import io.wolftown.kaiku.data.auth.DeviceSignInResult
import io.wolftown.kaiku.data.keydevice.KeyDevicePhase
import io.wolftown.kaiku.data.keydevice.KeyDeviceProbe
import io.wolftown.kaiku.data.keydevice.KeyDeviceRepository
import io.wolftown.kaiku.data.keydevice.KeyDeviceState
import io.wolftown.kaiku.data.keydevice.LocalKeyDevice
import io.wolftown.kaiku.data.keydevice.ServerKeyState
import io.wolftown.kaiku.domain.model.EnrollmentState
import io.wolftown.kaiku.ui.auth.signInResultText
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import java.io.IOException
import javax.inject.Inject
import kotlin.coroutines.cancellation.CancellationException

/**
 * L2-A13: a setup that waits (24 h for an existing account, a claim whose code
 * was not confirmed before the app closed, or a claim whose answer never
 * arrived). Survives restarts: the state lives in [KeyDeviceRepository].
 * [check] runs at every app open and every 15 minutes in the foreground;
 * active → device-key sign-in, cancelled or void → "Setup cancelled" and the
 * key is forgotten. With a claim secret it asks the enrolment status; without
 * one (a self-claim, an unanswered claim) it asks the device-login probe
 * ([KeyDeviceProbe]). Never a login form.
 */
@HiltViewModel
class SetupPendingViewModel @Inject constructor(
    private val enrollmentApi: EnrollmentApi,
    private val keyDevices: KeyDeviceRepository,
    private val deviceSignIn: DeviceSignIn,
    private val probe: KeyDeviceProbe,
) : ViewModel() {

    private val _ui = MutableStateFlow(initial())
    val ui: StateFlow<SetupUi> = _ui.asStateFlow()
    private var signingIn = false
    private var checking = false

    private fun initial(): SetupUi {
        val s = keyDevices.local() ?: return SetupUi.Ended(R.string.setup_cancelled, canRetry = false)
        return when (keyDevices.phase()) {
            KeyDevicePhase.PENDING -> (keyDevices.current() as? KeyDeviceState.Pending)
                ?.let { SetupUi.Pending(it.readyAt) } ?: SetupUi.WaitingForCode
            KeyDevicePhase.ACTIVE -> SetupUi.SigningIn
            KeyDevicePhase.CLAIMED -> if (s.claimSecret != null) SetupUi.WaitingForCode else SetupUi.Opening
            KeyDevicePhase.CLAIMING, null -> SetupUi.Opening
        }
    }

    fun check() {
        val local = keyDevices.local() ?: return
        val now = _ui.value
        // A paused sign-in waits for its button: a resume after the cancelled prompt never re-prompts.
        if (now == SetupUi.SigningIn || now == SetupUi.SignInPaused || now == SetupUi.Done ||
            (now is SetupUi.Ended && !now.canRetry)
        ) return
        if (checking) return
        checking = true
        viewModelScope.launch {
            try {
                val secret = local.claimSecret
                // Only a claim waiting for its code asks the enrolment; a pending (or unanswered)
                // key device asks the key-device probe, which outlives the finished enrolment.
                if (secret != null && keyDevices.phase() == KeyDevicePhase.CLAIMED) waitForCode(local, secret) else checkServer(local)
            } finally {
                checking = false
            }
        }
    }

    /** CLAIMED: long-polls the enrolment while the screen waits for the code to be confirmed. */
    private suspend fun waitForCode(local: LocalKeyDevice, secret: String) {
        do {
            if (!checkStatus(local, secret, LongPoll.WAIT_SECONDS)) return
        } while (_ui.value == SetupUi.WaitingForCode && keyDevices.phase() == KeyDevicePhase.CLAIMED)
    }

    /** One status read; false when it could not be read (offline: the next check tries again). */
    private suspend fun checkStatus(local: LocalKeyDevice, secret: String, wait: Int): Boolean {
        try {
            val s = enrollmentApi.status(local.enrollmentId, secret, wait, local.origin)
            when (s.state) {
                EnrollmentState.DONE -> if (s.authenticatorState == "pending") {
                    val at = parseInstant(s.readyAt)
                    if (at != null) {
                        keyDevices.markPending(local, at)
                        _ui.value = SetupUi.Pending(at)
                    } else {
                        checkServer(local)
                    }
                } else {
                    keyDevices.markEnrolled(local)
                    _ui.value = SetupUi.SigningIn
                }
                EnrollmentState.CANCELLED -> forget(R.string.setup_cancelled)
                EnrollmentState.EXPIRED -> forget(R.string.setup_expired)
                EnrollmentState.FAILED -> forget(R.string.setup_code_failed)
                else -> Unit
            }
            return true
        } catch (c: CancellationException) {
            throw c
        } catch (x: ApiException) {
            // An unknown or void enrolment: the setup is over.
            if (x.status.value == 404 || x.status.value == 410) forget(R.string.setup_cancelled)
            return false
        } catch (_: Exception) {
            // Offline or an unreadable answer: try again at the next check.
            return false
        }
    }

    /** No claim secret to ask with: the server's view of this phone's key device decides. */
    private suspend fun checkServer(local: LocalKeyDevice) {
        val claiming = keyDevices.phase() == KeyDevicePhase.CLAIMING
        when (val st = probe.probe(local.userId, local.origin)) {
            ServerKeyState.Active -> {
                keyDevices.markEnrolled(local)
                _ui.value = SetupUi.SigningIn
            }
            is ServerKeyState.Pending -> {
                keyDevices.markPending(local, st.readyAt)
                _ui.value = SetupUi.Pending(st.readyAt)
            }
            // An unanswered claim that never became a key (or whose code was never confirmed) ends here.
            ServerKeyState.Gone -> forget(if (claiming) R.string.setup_not_finished else R.string.setup_cancelled)
            ServerKeyState.Unknown -> if (_ui.value !is SetupUi.Pending) {
                _ui.value = SetupUi.Ended(R.string.security_network_error, canRetry = true)
            }
        }
    }

    fun signIn(activity: FragmentActivity) {
        val now = _ui.value
        if ((now != SetupUi.SigningIn && now != SetupUi.SignInPaused) || signingIn) return
        signingIn = true
        _ui.value = SetupUi.SigningIn
        viewModelScope.launch {
            val r = deviceSignIn.signIn(activity)
            signingIn = false
            _ui.value = when (r) {
                DeviceSignInResult.Success -> SetupUi.Done
                DeviceSignInResult.Cancelled -> SetupUi.SignInPaused
                is DeviceSignInResult.Pending -> SetupUi.Pending(r.readyAt)
                else -> SetupUi.Ended(signInResultText(r) ?: R.string.setup_failed_generic, canRetry = false)
            }
        }
    }

    private fun forget(@StringRes msg: Int) {
        keyDevices.forget()
        _ui.value = SetupUi.Ended(msg, canRetry = false)
    }
}
