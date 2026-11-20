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
import io.wolftown.kaiku.data.auth.RemovalResult
import io.wolftown.kaiku.data.auth.SessionEndLog
import io.wolftown.kaiku.data.auth.SignInPurpose
import io.wolftown.kaiku.data.keydevice.Ask
import io.wolftown.kaiku.data.keydevice.KeyAnswers
import io.wolftown.kaiku.data.keydevice.KeyDeviceProbe
import io.wolftown.kaiku.data.keydevice.KeyDeviceRepository
import io.wolftown.kaiku.data.keydevice.ServerKeyState
import io.wolftown.kaiku.data.keydevice.ServerSilence
import io.wolftown.kaiku.data.keydevice.KeyDeviceState
import io.wolftown.kaiku.domain.KaikuPayloads
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeoutOrNull
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
    /**
     * The key's server has stayed silent for long ([ServerSilence.lasting]: several separate
     * occasions over at least a day): the screen names the last resort, in words only.
     */
    val serverSilent: Boolean = false,
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
    is DeviceSignInResult.Failed -> when (r.code) {
        "UV_LOCKED_OUT" -> R.string.security_locked_out
        // The server's word about this install was about another key; the setup is back where it was.
        "NOT_THIS_KEY" -> R.string.setup_not_this_key
        // The install id this key was enrolled under is lost: the key is kept, nothing changed.
        "INSTALL_ID_LOST" -> R.string.confirm_install_unknown
        else -> R.string.security_sign_in_failed
    }
}

/**
 * What "Confirm it's you" says when "Use another account" did not remove this phone. The only key
 * of an account is never removed by its owner (`LAST_STRONG_AUTHENTICATOR`): the phone stays the
 * key, signed out, and the text names the ways on (add another phone or passkey, or the admin).
 */
@StringRes
internal fun removalFailedText(out: StepUpOutcome): Int = removalFailedText((out as? StepUpOutcome.Failed)?.code)

@StringRes
internal fun removalFailedText(code: String?): Int {
    return when (code) {
        "KEY_INVALIDATED" -> R.string.security_key_invalidated
        "LAST_STRONG_AUTHENTICATOR" -> R.string.security_only_key
        "UV_LOCKED_OUT" -> R.string.security_locked_out
        "NETWORK" -> R.string.security_network_error
        "APPROVAL_EXPIRED", "TOOK_TOO_LONG" -> R.string.security_took_too_long
        "ACCOUNT_BANNED" -> R.string.auth_login_banned
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
    private val answers: KeyAnswers,
    private val silence: ServerSilence,
) : ViewModel() {

    companion object {
        /** The longest "Use another account" may take (two prompts, each on a challenge of two minutes). */
        const val REMOVAL_TIMEOUT_MS = 300_000L

        /** The longest "Confirm it's you" may take: its requests and the prompt, whose challenge lasts 120 s. */
        const val SIGN_IN_TIMEOUT_MS = 180_000L
    }

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
        val rec = keyDevices.record.value
        if (rec != null && keyDevices.current() is KeyDeviceState.Lapsed) {
            viewModelScope.launch {
                when (val st = probe.probe(rec)) {
                    is ServerKeyState.Gone ->
                        if (!_ui.value.working && keyDevices.current() is KeyDeviceState.Lapsed &&
                            answers.settle(rec, KaikuPayloads.canonicalOrigin(rec.device.origin), st).forgotten
                        ) {
                            _ui.update { it.copy(error = R.string.security_device_revoked, keyGone = true) }
                        }
                    // No answer of the server itself. Noted with the key; once that has lasted, the
                    // screen names the last resort (nothing here deletes the key).
                    ServerKeyState.Unknown -> {
                        val noted = silence.noteUnanswered(rec)
                        if (silence.lasting(noted)) _ui.update { it.copy(serverSilent = true) }
                    }
                    // The server is there (also when it refuses): nothing is noted any more.
                    ServerKeyState.Active, is ServerKeyState.Pending, ServerKeyState.Refused -> silence.noteAnswered(rec)
                }
            }
        }
    }

    /** [stayingSignedIn]: the renewal before the 90-day cap (its prompt says so). */
    fun confirm(activity: FragmentActivity, stayingSignedIn: Boolean = false) {
        if (_ui.value.working || _ui.value.keyGone) return
        _ui.update { it.copy(working = true, error = null) }
        viewModelScope.launch {
            val r = withTimeoutOrNull(SIGN_IN_TIMEOUT_MS) {
                deviceSignIn.signIn(activity, if (stayingSignedIn) SignInPurpose.STAY_SIGNED_IN else SignInPurpose.SIGN_IN)
            } ?: if (!stayingSignedIn && keyDevices.current() is KeyDeviceState.Active) {
                // It got through just as the limit hit (a renewal runs signed in: it cannot tell).
                DeviceSignInResult.Success
            } else {
                DeviceSignInResult.Expired
            }
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
     * "Use another account": this phone stops being the key first. Removing it needs a session. A
     * phone that is not signed in gets one that only the removal itself holds
     * ([io.wolftown.kaiku.data.auth.PhoneRemoval]): the app is never signed in by it, nothing of it
     * is stored, and when this is stopped half way (the screen goes away) there is nothing to undo.
     */
    fun removeThisPhone(activity: FragmentActivity) {
        if (_ui.value.working || _ui.value.keyGone) return
        _ui.update { it.copy(working = true, error = null) }
        viewModelScope.launch {
            if (keyDevices.current() is KeyDeviceState.Active) {
                val out = signOut.signOutAndRemovePhone(activity)
                val keyGone = out is StepUpOutcome.Failed && out.code == "KEY_INVALIDATED"
                _ui.update {
                    when (out) {
                        is StepUpOutcome.Granted -> it.copy(working = false, done = ConfirmDone.SIGNED_OUT)
                        StepUpOutcome.Cancelled -> it.copy(working = false)
                        else -> it.copy(working = false, keyGone = keyGone, error = removalFailedText(out))
                    }
                }
            } else {
                val r = withTimeoutOrNull(REMOVAL_TIMEOUT_MS) { signOut.removeSignedOutPhone(activity) }
                    ?: RemovalResult.Failed("TOOK_TOO_LONG")
                _ui.update {
                    when (r) {
                        RemovalResult.Removed -> it.copy(working = false, done = ConfirmDone.SIGNED_OUT)
                        RemovalResult.Cancelled -> it.copy(working = false)
                        is RemovalResult.KeyGone -> it.copy(
                            working = false,
                            keyGone = true,
                            error = if (r.invalidated) R.string.security_key_invalidated else R.string.security_device_revoked,
                        )
                        is RemovalResult.Failed -> it.copy(working = false, error = removalFailedText(r.code))
                    }
                }
            }
            recheckKey()
        }
    }

    /** The key may have been forgotten while an action ran (the record observer waits for none). */
    private fun recheckKey() {
        val u = _ui.value
        if (!u.working && !u.keyGone && u.done == null && keyDevices.local() == null) {
            _ui.update { it.copy(error = R.string.security_device_revoked, keyGone = true) }
        }
    }

    /** After the key is gone: the app continues as unconfigured for this account. */
    fun acknowledgeKeyGone() {
        _ui.update { it.copy(done = ConfirmDone.SIGNED_OUT) }
    }
}
