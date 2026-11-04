package io.wolftown.kaiku.ui.auth

import androidx.annotation.StringRes
import androidx.fragment.app.FragmentActivity
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import dagger.hilt.android.lifecycle.HiltViewModel
import io.wolftown.kaiku.R
import io.wolftown.kaiku.approval.StepUpOutcome
import io.wolftown.kaiku.data.auth.DeviceSignIn
import io.wolftown.kaiku.data.auth.DeviceSignInResult
import io.wolftown.kaiku.data.auth.KeyDeviceSignOut
import io.wolftown.kaiku.data.auth.SessionEndLog
import io.wolftown.kaiku.data.keydevice.KeyDeviceRepository
import io.wolftown.kaiku.data.keydevice.KeyDeviceState
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import java.time.Instant
import javax.inject.Inject

enum class ConfirmDone { SIGNED_IN, SIGNED_OUT, PENDING }

data class ConfirmItsYouUi(
    val accountName: String? = null,
    @StringRes val notice: Int? = null,
    val working: Boolean = false,
    @StringRes val error: Int? = null,
    /** Shown with [error] when the phone is no longer a key; the only way on is "OK". */
    val keyGone: Boolean = false,
    val readyAt: Instant? = null,
    val done: ConfirmDone? = null,
)

/** What "Confirm it's you" says after a device sign-in that did not succeed. */
@StringRes
internal fun signInResultText(r: DeviceSignInResult): Int? = when (r) {
    DeviceSignInResult.Success, DeviceSignInResult.Cancelled, is DeviceSignInResult.Pending -> null
    DeviceSignInResult.Revoked -> R.string.security_device_revoked
    DeviceSignInResult.KeyInvalidated -> R.string.security_key_invalidated
    DeviceSignInResult.Banned -> R.string.auth_login_banned
    DeviceSignInResult.Network -> R.string.security_network_error
    is DeviceSignInResult.Failed ->
        if (r.code == "UV_LOCKED_OUT") R.string.security_locked_out else R.string.security_sign_in_failed
}

/** L2-A11: the lapsed key device signs in again with its screen lock, never a password. */
@HiltViewModel
class ConfirmItsYouViewModel @Inject constructor(
    private val deviceSignIn: DeviceSignIn,
    private val keyDevices: KeyDeviceRepository,
    private val signOut: KeyDeviceSignOut,
    sessionEndLog: SessionEndLog,
) : ViewModel() {

    private val _ui = MutableStateFlow(
        ConfirmItsYouUi(
            accountName = keyDevices.local()?.accountName,
            notice = if (sessionEndLog.consume() == "SESSION_REUSED") R.string.security_session_reused else null,
        ),
    )
    val ui: StateFlow<ConfirmItsYouUi> = _ui.asStateFlow()

    fun confirm(activity: FragmentActivity) {
        if (_ui.value.working) return
        _ui.update { it.copy(working = true, error = null) }
        viewModelScope.launch {
            val r = deviceSignIn.signIn(activity)
            _ui.update {
                it.copy(
                    working = false,
                    error = signInResultText(r),
                    keyGone = r == DeviceSignInResult.Revoked || r == DeviceSignInResult.KeyInvalidated,
                    readyAt = (r as? DeviceSignInResult.Pending)?.readyAt,
                    done = when (r) {
                        DeviceSignInResult.Success -> ConfirmDone.SIGNED_IN
                        is DeviceSignInResult.Pending -> ConfirmDone.PENDING
                        else -> null
                    },
                )
            }
        }
    }

    /**
     * "Use another account": this phone stops being the key first. Removing it
     * needs a session, so it signs in with the key, then steps up and removes.
     */
    fun removeThisPhone(activity: FragmentActivity) {
        if (_ui.value.working) return
        _ui.update { it.copy(working = true, error = null) }
        viewModelScope.launch {
            val signedInForRemoval = keyDevices.current() !is KeyDeviceState.Active
            if (signedInForRemoval) {
                val r = deviceSignIn.signIn(activity)
                if (r != DeviceSignInResult.Success) {
                    _ui.update {
                        it.copy(
                            working = false,
                            error = signInResultText(r),
                            keyGone = r == DeviceSignInResult.Revoked || r == DeviceSignInResult.KeyInvalidated,
                            readyAt = (r as? DeviceSignInResult.Pending)?.readyAt,
                            done = if (r is DeviceSignInResult.Pending) ConfirmDone.PENDING else null,
                        )
                    }
                    return@launch
                }
            }
            val out = signOut.signOutAndRemovePhone(activity)
            // The session made only for the removal never outlives a removal that did not happen:
            // the phone goes back to "Confirm it's you" (or, without a key, to signed out).
            if (signedInForRemoval && out !is StepUpOutcome.Granted) signOut.signOut()
            val keyGone = out is StepUpOutcome.Failed && out.code == "KEY_INVALIDATED"
            _ui.update {
                when (out) {
                    is StepUpOutcome.Granted -> it.copy(working = false, done = ConfirmDone.SIGNED_OUT)
                    StepUpOutcome.Cancelled -> it.copy(working = false)
                    else -> it.copy(
                        working = false,
                        keyGone = keyGone,
                        error = when {
                            keyGone -> R.string.security_key_invalidated
                            out is StepUpOutcome.Failed && out.code == "UV_LOCKED_OUT" -> R.string.security_locked_out
                            out is StepUpOutcome.Failed && out.code == "NETWORK" -> R.string.security_network_error
                            else -> R.string.security_sign_in_failed
                        },
                    )
                }
            }
        }
    }

    /** After the key is gone: the app continues as unconfigured for this account. */
    fun acknowledgeKeyGone() {
        _ui.update { it.copy(done = ConfirmDone.SIGNED_OUT) }
    }
}
