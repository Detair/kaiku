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
import io.wolftown.kaiku.data.auth.RemovalsInProgress
import io.wolftown.kaiku.data.auth.SessionEndLog
import io.wolftown.kaiku.data.auth.SignInPurpose
import io.wolftown.kaiku.data.keydevice.KeyDeviceProbe
import io.wolftown.kaiku.data.keydevice.KeyDeviceRepository
import io.wolftown.kaiku.data.keydevice.ServerKeyState
import io.wolftown.kaiku.data.keydevice.KeyDeviceState
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
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
    DeviceSignInResult.Expired -> R.string.security_took_too_long
    is DeviceSignInResult.Failed ->
        if (r.code == "UV_LOCKED_OUT") R.string.security_locked_out else R.string.security_sign_in_failed
}

/**
 * What "Confirm it's you" says when "Use another account" did not remove this phone. The only key
 * of an account is never removed by its owner (`LAST_STRONG_AUTHENTICATOR`): the phone stays the
 * key, signed out, and the text names the ways on (add another phone or passkey, or the admin).
 */
@StringRes
internal fun removalFailedText(out: StepUpOutcome): Int {
    val code = (out as? StepUpOutcome.Failed)?.code
    return when (code) {
        "KEY_INVALIDATED" -> R.string.security_key_invalidated
        "LAST_STRONG_AUTHENTICATOR" -> R.string.security_only_key
        "UV_LOCKED_OUT" -> R.string.security_locked_out
        "NETWORK" -> R.string.security_network_error
        "APPROVAL_EXPIRED" -> R.string.security_took_too_long
        else -> R.string.security_sign_in_failed
    }
}

/** The sign-in found no key on this phone any more (removed, invalidated, or forgotten meanwhile). */
private fun DeviceSignInResult.keyIsGone(): Boolean =
    this == DeviceSignInResult.Revoked || this == DeviceSignInResult.KeyInvalidated ||
        (this is DeviceSignInResult.Failed && code == "NO_KEY_DEVICE")

@StringRes
private fun gonePhoneText(r: DeviceSignInResult): Int =
    if (r == DeviceSignInResult.KeyInvalidated) R.string.security_key_invalidated else R.string.security_device_revoked

/** L2-A11: the lapsed key device signs in again with its screen lock, never a password. */
@HiltViewModel
class ConfirmItsYouViewModel @Inject constructor(
    private val deviceSignIn: DeviceSignIn,
    private val keyDevices: KeyDeviceRepository,
    private val signOut: KeyDeviceSignOut,
    sessionEndLog: SessionEndLog,
    probe: KeyDeviceProbe,
    private val removals: RemovalsInProgress = RemovalsInProgress(),
) : ViewModel() {

    /** Held while "Use another account" runs here; an app start then leaves its session to it. */
    private var removal: AutoCloseable? = null

    private val _ui = MutableStateFlow(
        ConfirmItsYouUi(
            accountName = keyDevices.local()?.accountName,
            notice = if (sessionEndLog.consume() == "SESSION_REUSED") R.string.security_session_reused else null,
        ),
    )
    val ui: StateFlow<ConfirmItsYouUi> = _ui.asStateFlow()

    init {
        // The key can be forgotten while this screen is open (a setup link on top found it removed,
        // or another screen did): then only "OK" leads on, never a button that cannot work.
        viewModelScope.launch {
            keyDevices.record.collect { r ->
                if (r == null && !_ui.value.working && !_ui.value.keyGone && _ui.value.done == null) {
                    _ui.update { u -> u.copy(error = R.string.security_device_revoked, keyGone = true) }
                }
            }
        }
        // An admin may have removed this phone as a key while it was signed out. The key's own
        // server is asked once (the public device-login challenge, left unanswered); only a key it
        // does not know, or knows as removed, is forgotten. Offline or unclear keeps everything.
        val local = keyDevices.local()
        if (local != null && keyDevices.current() is KeyDeviceState.Lapsed) {
            viewModelScope.launch {
                val gone = probe.probe(local.userId, local.origin) == ServerKeyState.Gone
                if (gone && !_ui.value.working && keyDevices.local() == local && keyDevices.current() is KeyDeviceState.Lapsed) {
                    keyDevices.forget()
                    _ui.update { it.copy(error = R.string.security_device_revoked, keyGone = true) }
                }
            }
        }
    }

    /** [stayingSignedIn]: the renewal before the 90-day cap (its prompt says so). */
    fun confirm(activity: FragmentActivity, stayingSignedIn: Boolean = false) {
        if (_ui.value.working || _ui.value.keyGone) return
        _ui.update { it.copy(working = true, error = null) }
        viewModelScope.launch {
            val r = deviceSignIn.signIn(activity, if (stayingSignedIn) SignInPurpose.STAY_SIGNED_IN else SignInPurpose.SIGN_IN)
            _ui.update {
                it.copy(
                    working = false,
                    error = if (r.keyIsGone()) gonePhoneText(r) else signInResultText(r),
                    keyGone = r.keyIsGone(),
                    readyAt = (r as? DeviceSignInResult.Pending)?.readyAt,
                    done = when (r) {
                        DeviceSignInResult.Success -> ConfirmDone.SIGNED_IN
                        is DeviceSignInResult.Pending -> ConfirmDone.PENDING
                        else -> null
                    },
                )
            }
            recheckKey()
        }
    }

    /**
     * "Use another account": this phone stops being the key first. Removing it needs a session, so
     * a signed-out phone signs in with the key, then steps up and removes.
     *
     * The session made only for the removal never outlives a removal that did not happen, also when
     * this is stopped half way (the screen goes away while the app stays): the clean-up cannot be
     * cancelled, signs that session out and clears the mark. If the app dies first, the next activity
     * start does it ([io.wolftown.kaiku.data.auth.UnfinishedRemoval]).
     */
    fun removeThisPhone(activity: FragmentActivity) {
        if (_ui.value.working || _ui.value.keyGone) return
        _ui.update { it.copy(working = true, error = null) }
        val signedInForRemoval = keyDevices.current() !is KeyDeviceState.Active
        val running = if (signedInForRemoval) removals.begin().also { removal = it } else null
        viewModelScope.launch {
            var sessionMade = false
            var granted = false
            var cleaned = !signedInForRemoval
            suspend fun cleanUp() {
                if (cleaned) return
                cleaned = true
                running?.close()
                withContext(NonCancellable) {
                    // Signed in with the key now can only be the removal's own session.
                    if (!granted && (sessionMade || keyDevices.current() is KeyDeviceState.Active)) signOut.signOut()
                    keyDevices.markRemovalSignIn(false)
                }
            }
            try {
                if (signedInForRemoval) {
                    // Stored before the sign-in: if the app dies before the removal ends, the next
                    // start ends this session before anything reads it.
                    keyDevices.markRemovalSignIn(true)
                    val r = deviceSignIn.signIn(activity, SignInPurpose.FOR_REMOVAL)
                    sessionMade = r == DeviceSignInResult.Success
                    if (!sessionMade) {
                        cleanUp()
                        _ui.update {
                            it.copy(
                                working = false,
                                error = if (r.keyIsGone()) gonePhoneText(r) else signInResultText(r),
                                keyGone = r.keyIsGone(),
                                readyAt = (r as? DeviceSignInResult.Pending)?.readyAt,
                                done = if (r is DeviceSignInResult.Pending) ConfirmDone.PENDING else null,
                            )
                        }
                        recheckKey()
                        return@launch
                    }
                }
                val out = signOut.signOutAndRemovePhone(activity)
                granted = out is StepUpOutcome.Granted
                // The phone goes back to "Confirm it's you" (or, without a key, to signed out).
                cleanUp()
                val keyGone = out is StepUpOutcome.Failed && out.code == "KEY_INVALIDATED"
                _ui.update {
                    when (out) {
                        is StepUpOutcome.Granted -> it.copy(working = false, done = ConfirmDone.SIGNED_OUT)
                        StepUpOutcome.Cancelled -> it.copy(working = false)
                        else -> it.copy(working = false, keyGone = keyGone, error = removalFailedText(out))
                    }
                }
                recheckKey()
            } finally {
                cleanUp()
            }
        }
    }

    /** The key may have been forgotten while an action ran (the record observer waits for none). */
    private fun recheckKey() {
        val u = _ui.value
        if (!u.working && !u.keyGone && u.done == null && keyDevices.local() == null) {
            _ui.update { it.copy(error = R.string.security_device_revoked, keyGone = true) }
        }
    }

    override fun onCleared() {
        // The screen is gone: an app start from now on ends a removal session left behind.
        removal?.close()
        super.onCleared()
    }

    /** After the key is gone: the app continues as unconfigured for this account. */
    fun acknowledgeKeyGone() {
        _ui.update { it.copy(done = ConfirmDone.SIGNED_OUT) }
    }
}
