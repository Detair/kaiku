package io.wolftown.kaiku.ui.auth.setup

import android.os.Build
import androidx.annotation.StringRes
import androidx.fragment.app.FragmentActivity
import androidx.lifecycle.ViewModel
import dagger.hilt.android.lifecycle.HiltViewModel
import io.wolftown.kaiku.R
import io.wolftown.kaiku.approval.UnsuitableReason
import io.wolftown.kaiku.approval.UvDecision
import io.wolftown.kaiku.data.api.ApiException
import io.wolftown.kaiku.data.auth.DeviceSignInResult
import io.wolftown.kaiku.data.auth.OriginPin
import io.wolftown.kaiku.data.keydevice.KeyDeviceRepository
import io.wolftown.kaiku.data.local.AuthSession
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.data.local.TokenStorage
import io.wolftown.kaiku.domain.KaikuPayloads
import io.wolftown.kaiku.ui.auth.signInResultText
import kotlinx.coroutines.flow.StateFlow
import java.io.IOException
import java.time.Instant
import javax.inject.Inject

sealed interface SetupUi {
    data object Opening : SetupUi
    data class ConfirmServer(val serverName: String, val host: String) : SetupUi
    data class ScreenLockNeeded(val reason: UnsuitableReason) : SetupUi
    /**
     * [username] is the signed `subject_username` and [host] the pinned origin's host: the
     * primary lines (C26: show only signed values); [displayName] is a secondary hint.
     */
    data class ConfirmSubject(val displayName: String, val username: String, val serverName: String, val host: String = "") : SetupUi
    /** [code] already grouped ("123 456"). */
    data class ShowCode(val code: String, val expiresAt: Instant?) : SetupUi
    /** A claim waits for its confirmation after an app restart (the code is never stored). */
    data object WaitingForCode : SetupUi
    data class Pending(val readyAt: Instant) : SetupUi
    /** The signed claim is on its way; its answer carries the code, so nothing interrupts it. */
    data object Claiming : SetupUi
    /** Active: the screen starts the device-key sign-in (it needs the activity for UV). */
    data object SigningIn : SetupUi
    /**
     * A claim's answer never came and the server does not list this phone as a key yet: it is
     * asked again by itself once nothing can come of that claim any more (minutes).
     */
    data object ClaimWaiting : SetupUi
    /** The UV prompt of the final sign-in was cancelled: one button to try again. */
    data object SignInPaused : SetupUi
    /** The final sign-in's prompt stayed open past its challenge: say so, one button for a fresh one. */
    data object SignInTimedOut : SetupUi
    data object Done : SetupUi
    /**
     * The setup screen came back without its setup code (process death; the code is
     * kept in memory only) while this phone has a stored setup: the waiting screen takes over.
     */
    data object Resume : SetupUi
    /** The setup screen came back without its setup code before anything was claimed. */
    data object Interrupted : SetupUi
    /** [offerSignIn]: this phone is already a key of the account (C26 `ENROLLMENT_DEVICE_IN_USE`): sign in with it. */
    data class Ended(@StringRes val msg: Int, val canRetry: Boolean, val arg: String? = null, val offerSignIn: Boolean = false) : SetupUi
}

/** What the setup needs from the app around it (JVM-testable). */
interface SetupEnvironment {
    /** The origin a session or a key ties the app to; null = unconfigured. */
    fun pinnedOrigin(): String?
    fun signedInUserId(): String?
    /**
     * An unconfigured app adopts the setup code's server once the person confirmed it: only if,
     * at this moment, the app is still tied to no server and holds no key record (the same rule a
     * link is accepted under). False: nothing was written.
     */
    fun adoptServer(origin: String): Boolean
    /** The saved server URL (an unconfigured app may have one typed in, without a session). */
    fun savedServer(): String?
    /** Puts back what an abandoned setup replaced: the server saved before, or none (null). */
    fun restoreServer(url: String?)
    val deviceLabel: String
    val apiLevel: Int
}

class AndroidSetupEnvironment @Inject constructor(
    private val originPin: OriginPin,
    private val authState: AuthState,
    private val tokenStorage: TokenStorage,
    private val keyDevices: KeyDeviceRepository,
) : SetupEnvironment {
    override fun pinnedOrigin() = originPin.pinned()
    override fun signedInUserId() = (authState.session.value as? AuthSession.LoggedIn)?.userId
    override fun adoptServer(origin: String): Boolean {
        if (keyDevices.unreadable() || keyDevices.record.value != null || originPin.pinned() != null) return false
        if (tokenStorage.getServerUrl()?.let(KaikuPayloads::canonicalOrigin) != origin) tokenStorage.saveServerUrl(origin)
        return true
    }
    override fun savedServer(): String? = tokenStorage.getServerUrl()
    override fun restoreServer(url: String?) {
        // Exactly what was there before the setup adopted its server, also when that was nothing.
        if (url != null) tokenStorage.saveServerUrl(url) else tokenStorage.clearServerUrl()
    }
    override val deviceLabel: String = "${Build.MANUFACTURER} ${Build.MODEL}".trim()
    override val apiLevel: Int = Build.VERSION.SDK_INT
}

/**
 * A screen's view of the one setup ([SetupOwner]): the scanner screen and the link screen each
 * have one. It holds no setup of its own; every action goes to the owner, which decides.
 */
@HiltViewModel
class SetupFlowViewModel @Inject constructor(private val owner: SetupOwner) : ViewModel() {

    private val view = owner.attach()

    val ui: StateFlow<SetupUi> = owner.ui
    val linkIgnored: StateFlow<Boolean> = owner.linkIgnored
    val waiting: StateFlow<Boolean> = owner.waiting
    val scanReady: StateFlow<UvDecision?> = owner.scanReady
    val cannotAsk: StateFlow<Boolean> = owner.cannotAsk
    val discardable: StateFlow<Boolean> = owner.discardable
    val startedFlow: StateFlow<Boolean> = owner.startedFlow
    val started: Boolean get() = owner.started

    private var scannerEntered = false

    /** The scanner screen is shown: what the phone can do is checked anew, once per visit (a rotation is not a visit). */
    fun enterScanner() {
        if (scannerEntered) return
        scannerEntered = true
        owner.checkBeforeScan(fresh = true)
    }

    fun checkBeforeScan() = owner.checkBeforeScan()
    fun discard() = owner.discard()
    fun start(e: SetupEntry) = owner.start(e)
    fun resumeWithoutEntry() = owner.resumeWithoutEntry()
    fun reset() = owner.reset()
    fun retry() = owner.retry()
    fun cancel() = owner.cancel()
    fun confirmServer() = owner.confirmServer()
    fun recheckScreenLock() = owner.recheckScreenLock()
    fun confirmSubject(activity: FragmentActivity) = owner.confirmSubject(activity)
    fun signIn(activity: FragmentActivity) = owner.signIn(activity)

    public override fun onCleared() {
        view.close()
        super.onCleared()
    }

    companion object {
        const val CLAIM_TIMEOUT_MS = SetupOwner.CLAIM_TIMEOUT_MS
        const val SIGN_IN_TIMEOUT_MS = SetupOwner.SIGN_IN_TIMEOUT_MS
    }
}

/** The screen after the final sign-in of a setup; null: it did not end in time. */
internal fun finalSignInUi(r: DeviceSignInResult?): SetupUi = when (r) {
    null, DeviceSignInResult.Expired -> SetupUi.SignInTimedOut
    DeviceSignInResult.Success -> SetupUi.Done
    DeviceSignInResult.Cancelled -> SetupUi.SignInPaused
    is DeviceSignInResult.Pending -> SetupUi.Pending(r.readyAt)
    else -> SetupUi.Ended(signInResultText(r) ?: R.string.setup_failed_generic, canRetry = false)
}

/** What the Back gesture does on a setup step. */
enum class SetupBack {
    /** A question before anything binds: the same as its "Cancel". */
    CANCEL,
    /** A question about the server or the account, or the code is out (or awaited): ask before leaving. */
    ASK,
    /** A prompt or the final sign-in runs: Back does nothing. */
    STAY,
    /** An ended step: leave as usual. */
    LEAVE,
}

internal fun setupBackAction(ui: SetupUi): SetupBack = when (ui) {
    is SetupUi.ShowCode, SetupUi.WaitingForCode, is SetupUi.ConfirmServer, is SetupUi.ConfirmSubject,
    is SetupUi.ScreenLockNeeded -> SetupBack.ASK
    SetupUi.Opening -> SetupBack.CANCEL
    SetupUi.Claiming, SetupUi.SigningIn, SetupUi.Done -> SetupBack.STAY
    else -> SetupBack.LEAVE
}

/** Back on the waiting screen: asks while the code is awaited, waits while the final sign-in runs. */
internal fun pendingBackAction(ui: SetupUi): SetupBack = when (setupBackAction(ui)) {
    SetupBack.ASK -> SetupBack.ASK
    SetupBack.STAY -> SetupBack.STAY
    SetupBack.CANCEL, SetupBack.LEAVE -> SetupBack.LEAVE
}

/** Server refusals during setup (C26) as texts. */
internal fun setupErrorFor(x: Exception): SetupUi.Ended = when (x) {
    is ApiException -> when (x.normalizedCode) {
        "ENROLLMENT_ALREADY_CLAIMED" -> SetupUi.Ended(R.string.setup_already_used, canRetry = false)
        "ENROLLMENT_TOKEN_INVALID" -> SetupUi.Ended(R.string.setup_token_invalid, canRetry = false)
        "DEVICE_UNSUITABLE" -> SetupUi.Ended(R.string.setup_cannot_be_key, canRetry = false)
        "ENROLLMENT_STATE", "APPROVAL_EXPIRED" -> SetupUi.Ended(R.string.setup_expired, canRetry = false)
        "TOO_MANY_ATTEMPTS" -> SetupUi.Ended(R.string.errors_rate_limited, canRetry = true)
        // C26/C27 amendment (2026-11-03): nothing was revoked; this phone already is that account's key.
        "ENROLLMENT_DEVICE_IN_USE" -> SetupUi.Ended(R.string.setup_device_in_use, canRetry = false, offerSignIn = true)
        "REGISTRATION_TICKET_INVALID" -> SetupUi.Ended(R.string.setup_registration_expired, canRetry = false)
        "ACCOUNT_REQUEST_NOT_READY" -> SetupUi.Ended(
            if (x.fields.state == "expired") R.string.setup_request_expired else R.string.setup_request_not_ready,
            canRetry = false,
        )
        else -> SetupUi.Ended(R.string.setup_failed_generic, canRetry = true)
    }
    is IOException -> SetupUi.Ended(R.string.security_network_error, canRetry = true)
    else -> SetupUi.Ended(R.string.setup_failed_generic, canRetry = true)
}

internal fun parseInstant(s: String?): Instant? = s?.let { runCatching { Instant.parse(it) }.getOrNull() }
