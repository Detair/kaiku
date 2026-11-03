package io.wolftown.kaiku.ui.auth.setup

import androidx.fragment.app.FragmentActivity
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import dagger.hilt.android.lifecycle.HiltViewModel
import io.wolftown.kaiku.R
import io.wolftown.kaiku.data.api.ApiException
import io.wolftown.kaiku.data.api.EnrollmentApi
import io.wolftown.kaiku.data.auth.DeviceSignIn
import io.wolftown.kaiku.data.auth.DeviceSignInResult
import io.wolftown.kaiku.data.keydevice.KeyDevicePhase
import io.wolftown.kaiku.data.keydevice.KeyDeviceRepository
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
 * L2-A13: a setup that waits (24 h for an existing account, or a claim whose
 * code was not confirmed before the app closed). Survives restarts: the state
 * lives in [KeyDeviceRepository]. [check] runs at every app open and every
 * 15 minutes in the foreground; active → device-key sign-in, cancelled or
 * void → "Setup cancelled" and the key is forgotten. Never a login form.
 */
@HiltViewModel
class SetupPendingViewModel @Inject constructor(
    private val enrollmentApi: EnrollmentApi,
    private val keyDevices: KeyDeviceRepository,
    private val deviceSignIn: DeviceSignIn,
) : ViewModel() {

    private val _ui = MutableStateFlow(initial())
    val ui: StateFlow<SetupUi> = _ui.asStateFlow()
    private var signingIn = false

    private fun initial(): SetupUi {
        val s = keyDevices.local() ?: return SetupUi.Ended(R.string.setup_cancelled, canRetry = false)
        return when (keyDevices.phase()) {
            KeyDevicePhase.PENDING -> (keyDevices.current() as? io.wolftown.kaiku.data.keydevice.KeyDeviceState.Pending)
                ?.let { SetupUi.Pending(it.readyAt) } ?: SetupUi.WaitingForCode
            KeyDevicePhase.ACTIVE -> SetupUi.SigningIn
            else -> if (s.claimSecret != null) SetupUi.WaitingForCode else SetupUi.Ended(R.string.setup_cancelled, canRetry = false)
        }
    }

    fun check() {
        val local = keyDevices.local() ?: return
        val secret = local.claimSecret ?: return
        if (_ui.value == SetupUi.SigningIn || _ui.value == SetupUi.Done || _ui.value is SetupUi.Ended) return
        viewModelScope.launch {
            try {
                val s = enrollmentApi.status(local.enrollmentId, secret, 0, local.origin)
                when (s.state) {
                    EnrollmentState.DONE -> if (s.authenticatorState == "pending") {
                        parseInstant(s.readyAt)?.let {
                            keyDevices.markPending(local, it)
                            _ui.value = SetupUi.Pending(it)
                        }
                    } else {
                        _ui.value = SetupUi.SigningIn
                    }
                    EnrollmentState.CANCELLED -> forget(R.string.setup_cancelled)
                    EnrollmentState.EXPIRED -> forget(R.string.setup_expired)
                    EnrollmentState.FAILED -> forget(R.string.setup_code_failed)
                    else -> Unit
                }
            } catch (c: CancellationException) {
                throw c
            } catch (x: ApiException) {
                // An unknown or void enrolment: the setup is over.
                if (x.status.value == 404 || x.status.value == 410) forget(R.string.setup_cancelled)
            } catch (_: IOException) {
                // Offline: try again at the next check.
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

    private fun forget(msg: Int) {
        keyDevices.forget()
        _ui.value = SetupUi.Ended(msg, canRetry = false)
    }
}
