package io.wolftown.kaiku.ui.auth

import android.content.Context
import android.net.Uri
import androidx.annotation.StringRes
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import dagger.hilt.android.lifecycle.HiltViewModel
import io.ktor.http.HttpStatusCode
import io.wolftown.kaiku.R
import io.wolftown.kaiku.data.api.ApiException
import io.wolftown.kaiku.data.api.LoginAttemptApi
import io.wolftown.kaiku.data.local.TokenStorage
import io.wolftown.kaiku.data.repository.AuthRepository
import io.wolftown.kaiku.di.AuthCoroutineScope
import io.wolftown.kaiku.domain.model.AttemptState
import io.wolftown.kaiku.domain.model.AuthResponse
import io.wolftown.kaiku.domain.model.LoginAttempt
import io.wolftown.kaiku.domain.model.LoginStep
import io.wolftown.kaiku.domain.model.LoginStepStatus
import io.wolftown.kaiku.domain.model.User
import java.io.IOException
import java.security.MessageDigest
import java.security.SecureRandom
import java.time.Duration
import java.time.Instant
import java.util.Base64
import javax.inject.Inject
import kotlin.coroutines.cancellation.CancellationException
import kotlin.math.ceil
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeoutOrNull

/** How the sign-in attempt on screen was started. */
enum class AttemptVia { OTHER_PHONE, PASSWORD, OIDC }

/** The phone sign-in screen (F2, F3, F4 on a phone that is not a key). */
sealed interface LoginAttemptUi {
    /**
     * The QR to scan with the key phone: F2 "Sign in with your other phone"
     * ([AttemptVia.OTHER_PHONE]) or F3's confirmation of a password or SSO
     * sign-in. [totpAvailable] offers "Phone not at hand?".
     */
    data class ShowQr(
        val payload: String,
        val minutesLeft: Int,
        val via: AttemptVia,
        val totpAvailable: Boolean = false,
    ) : LoginAttemptUi {
        // The payload carries the sign-in token: never in logs.
        override fun toString(): String = "ShowQr(minutesLeft=$minutesLeft, via=$via, totpAvailable=$totpAvailable)"
    }

    /** F4: the authenticator code for the same attempt. */
    data class Totp(@StringRes val error: Int?) : LoginAttemptUi

    /** The attempt ended without a session; [via] decides what "Try again" does. */
    data class Ended(@StringRes val msg: Int, val via: AttemptVia) : LoginAttemptUi
}

data class LoginAttemptState(
    /** Null while the login form shows. */
    val screen: LoginAttemptUi? = null,
    val busy: Boolean = false,
    /** A sign-in that never got an attempt (neutral answer, errors), shown on the form. */
    @StringRes val formError: Int? = null,
    /** `TOO_MANY_ATTEMPTS` with `Retry-After`: "Try again in N minutes" instead of [formError]. */
    val retryMinutes: Int? = null,
    val signedIn: Boolean = false,
    /** C4: the collected session must choose its own password first. */
    val passwordChangeRequired: Boolean = false,
)

/**
 * One attempt's collection secret (C27): a verifier of 32 random bytes, in
 * memory only, never shown, logged or saved; [challenge] = S256(verifier).
 */
class AttemptSecret private constructor(val verifier: String) {
    val challenge: String = b64u(MessageDigest.getInstance("SHA-256").digest(verifier.toByteArray(Charsets.US_ASCII)))

    override fun toString(): String = "AttemptSecret"

    companion object {
        private val random = SecureRandom()
        fun create(): AttemptSecret = AttemptSecret(b64u(ByteArray(32).also(random::nextBytes)))
        private fun b64u(b: ByteArray): String = Base64.getUrlEncoder().withoutPadding().encodeToString(b)
    }
}

/**
 * Signing in on a phone that is not (yet) a key (Task B14): the C27 attempt
 * machine (SM1) for "Sign in with your other phone" (F2), a password or SSO
 * sign-in confirmed by scanning with the key phone (F3), and the TOTP step (F4).
 * Each attempt draws its own verifier, which lives only in this ViewModel and
 * dies with the screen; leaving cancels the attempt. Every session it collects
 * is probation and goes through [AuthRepository.completeAttempt].
 */
@HiltViewModel
class LoginAttemptViewModel @Inject constructor(
    private val api: LoginAttemptApi,
    private val authRepository: AuthRepository,
    private val tokenStorage: TokenStorage,
    private val oidcHandler: OidcHandler,
    /** A cancel must reach the server after the ViewModel is cleared. */
    @AuthCoroutineScope private val appScope: CoroutineScope,
) : ViewModel() {

    companion object {
        /** Long-poll wait (C27 allows 0..25 s), below the shared client's read timeout ([LongPoll]). */
        const val POLL_WAIT_SECONDS = io.wolftown.kaiku.data.api.LongPoll.WAIT_SECONDS
        private const val CANCEL_TIMEOUT_MS = 5_000L
        private const val COLLECT_TRIES = 3

        /** Network retries: after 1 s, 2 s, then every 5 s while the attempt is live. */
        fun retryDelayMs(failures: Int): Long = when (failures) {
            0 -> 1_000L
            1 -> 2_000L
            else -> 5_000L
        }
    }

    /** What a password or SSO start sends before its [LoginStep] arrives. */
    private class PendingStart(val secret: AttemptSecret, val via: AttemptVia, val serverUrl: String, val password: String?)

    /** The attempt on screen. [password] only for a password attempt (C4). */
    private class Attempt(
        val id: String,
        val secret: AttemptSecret,
        val via: AttemptVia,
        val serverUrl: String,
        val password: String?,
    ) {
        var expiresAt: Instant = Instant.MAX
        var totpAvailable = false
        var collecting = false

        fun update(a: LoginAttempt) {
            runCatching { Instant.parse(a.expiresAt) }.getOrNull()?.let { expiresAt = it }
            totpAvailable = a.totpAvailable
        }

        fun live(): Boolean = Instant.now().isBefore(expiresAt)

        fun msLeft(): Long = if (expiresAt == Instant.MAX) Long.MAX_VALUE else Duration.between(Instant.now(), expiresAt).toMillis()

        fun minutesLeft(): Int =
            ceil(Duration.between(Instant.now(), expiresAt).seconds.coerceAtLeast(0) / 60.0).toInt()

        override fun toString(): String = "Attempt(id=$id, via=$via)"
    }

    private val _state = MutableStateFlow(LoginAttemptState())
    val state: StateFlow<LoginAttemptState> = _state.asStateFlow()

    private var current: Attempt? = null
    private var pendingStart: PendingStart? = null
    /** The SSO start's secret while the Custom Tab is open. */
    private var oidcSecret: AttemptSecret? = null
    private var stepJob: Job? = null
    private var pollJob: Job? = null

    init {
        // OIDC callbacks reach the screen that holds the verifier (MainActivity posts them).
        viewModelScope.launch {
            PendingOidcCallback.pending.collect { uri -> if (uri != null) PendingOidcCallback.take()?.let(::onOidcCallback) }
        }
    }

    /** F2 "Sign in with your other phone": a new `qr` attempt and its QR. */
    fun startQr() {
        val server = tokenStorage.getServerUrl() ?: return formFailure(IllegalStateException("no server"))
        val secret = AttemptSecret.create()
        begin {
            val attempt = api.createQr(secret.challenge)
            val a = Attempt(attempt.attemptId, secret, AttemptVia.OTHER_PHONE, server, null)
            current = a
            showQr(a, attempt)
        }
    }

    /** F3: username and password → a [LoginStep] (or F3's neutral answer on the form). */
    fun signInWithPassword(username: String, password: String) {
        val server = tokenStorage.getServerUrl() ?: return formFailure(IllegalStateException("no server"))
        val secret = AttemptSecret.create()
        begin {
            pendingStart = PendingStart(secret, AttemptVia.PASSWORD, server, password)
            applyStep(api.login(username, password, secret.challenge))
        }
    }

    /** F3 OIDC variant: the Custom Tab start carries this attempt's `client_challenge`. */
    fun startOidc(context: Context, providerSlug: String) {
        if (_state.value.busy || passkeyRunning) return
        dropAttempt()
        val secret = AttemptSecret.create()
        try {
            oidcHandler.launchOidcLogin(context, providerSlug, secret.challenge)
            oidcSecret = secret
            _state.update { it.copy(formError = null, retryMinutes = null) }
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            oidcSecret = null
            _state.update { it.copy(formError = AuthErrorMapper.oidc(e), retryMinutes = null) }
        }
    }

    /**
     * The `kaiku://auth/callback` of the SSO start: only a one-time code, redeemed
     * with this attempt's verifier. Tokens in the link, a link without a waiting
     * SSO sign-in or a provider error install nothing.
     */
    fun onOidcCallback(uri: Uri) {
        val secret = oidcSecret
        if (secret == null) {
            // Nobody waits for it (a stale or foreign link): a running sign-in is never touched.
            if (!_state.value.busy && _state.value.screen == null) {
                _state.update { it.copy(formError = R.string.auth_login_sso_failed, retryMinutes = null) }
            }
            return
        }
        oidcSecret = null
        val server = tokenStorage.getServerUrl()
        val callback = oidcHandler.handleCallback(uri)
        if (server == null || callback !is OidcCallback.Code) {
            _state.update { it.copy(formError = R.string.auth_login_sso_failed, retryMinutes = null, busy = false) }
            return
        }
        begin(map = AuthErrorMapper::oidc) {
            pendingStart = PendingStart(secret, AttemptVia.OIDC, server, null)
            applyStep(api.exchangeOidc(callback.code, secret.verifier))
        }
    }

    /**
     * The server's answer to a password or SSO start, for the verifier that
     * start sent: `confirmation_required` shows the confirmation QR,
     * `totp_required` the code field.
     */
    fun applyStep(step: LoginStep) {
        val start = pendingStart ?: return
        pendingStart = null
        val a = Attempt(step.attempt.attemptId, start.secret, start.via, start.serverUrl, start.password)
        current = a
        when (step.status) {
            LoginStepStatus.CONFIRMATION_REQUIRED -> showQr(a, step.attempt)
            LoginStepStatus.TOTP_REQUIRED -> {
                a.update(step.attempt)
                _state.update { it.copy(screen = LoginAttemptUi.Totp(error = null), busy = false) }
            }
            LoginStepStatus.UNKNOWN -> {
                dropAttempt()
                _state.update { it.copy(screen = null, busy = false, formError = R.string.auth_login_client_outdated) }
            }
        }
    }

    /** "Phone not at hand?": the same attempt continues with a TOTP code (F3 step 5). */
    fun phoneNotAtHand() {
        val a = current ?: return
        val qr = _state.value.screen as? LoginAttemptUi.ShowQr ?: return
        if (!qr.totpAvailable || a.via == AttemptVia.OTHER_PHONE) return
        pollJob?.cancel()
        _state.update { it.copy(screen = LoginAttemptUi.Totp(error = null)) }
    }

    fun submitTotp(code: String) {
        val a = current ?: return
        val digits = code.filter(Char::isDigit)
        if (digits.isEmpty() || _state.value.busy) return
        _state.update { it.copy(busy = true) }
        stepJob?.cancel()
        stepJob = viewModelScope.launch {
            val result = authRepository.completeAttempt(a.serverUrl, a.password) { api.totp(a.id, a.secret.verifier, digits) }
            finish(a, result, totp = true)
        }
    }

    /** "Try again" after an ended attempt: a new QR for F2, the form otherwise. */
    fun tryAgain() {
        val ended = _state.value.screen as? LoginAttemptUi.Ended ?: return
        if (ended.via == AttemptVia.OTHER_PHONE) startQr() else leave()
    }

    /** Back or closing the screen (SM1 "close the screen"): cancels the attempt. */
    fun leave() {
        dropAttempt()
        oidcSecret = null
        _state.update { it.copy(screen = null, busy = false) }
    }

    /** Called once the screen has navigated to `change_password`. */
    fun onPasswordChangeNavigated() {
        _state.update { it.copy(passwordChangeRequired = false) }
    }

    fun clearError() {
        _state.update { it.copy(formError = null, retryMinutes = null) }
    }

    override fun onCleared() {
        dropAttempt()
        super.onCleared()
    }

    /** A passkey sign-in runs ([LoginViewModel]); the screen reports it. Nothing starts beside it. */
    @Volatile
    private var passkeyRunning = false

    fun setPasskeyRunning(running: Boolean) {
        passkeyRunning = running
    }

    private fun begin(map: (Throwable) -> Int = AuthErrorMapper::login, block: suspend () -> Unit) {
        // One start at a time: a double tap, or a passkey sign-in already running, never sends a second.
        if (_state.value.busy || passkeyRunning) return
        dropAttempt()
        _state.update { it.copy(busy = true, screen = null, formError = null, retryMinutes = null) }
        stepJob = viewModelScope.launch {
            try {
                block()
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                pendingStart = null
                formFailure(e, map)
            }
        }
    }

    private fun formFailure(e: Throwable, map: (Throwable) -> Int = AuthErrorMapper::login) {
        val minutes = (e as? ApiException)
            ?.takeIf { it.status == HttpStatusCode.TooManyRequests }
            ?.fields?.retryAfterSeconds
            ?.let { ceil(it / 60.0).toInt().coerceAtLeast(1) }
        _state.update {
            it.copy(busy = false, screen = null, formError = if (minutes == null) map(e) else null, retryMinutes = minutes)
        }
    }

    private fun showQr(a: Attempt, attempt: LoginAttempt) {
        a.update(attempt)
        val totp = a.via != AttemptVia.OTHER_PHONE && a.totpAvailable
        _state.update {
            it.copy(
                screen = LoginAttemptUi.ShowQr(attempt.qr?.payload.orEmpty(), a.minutesLeft(), a.via, totp),
                busy = false,
            )
        }
        startPolling(a)
    }

    /** Long-poll until the attempt is approved (collect once) or ends (SM1; A3's poll loop). */
    private fun startPolling(a: Attempt) {
        pollJob?.cancel()
        pollJob = viewModelScope.launch {
            var failures = 0
            while (current === a) {
                val polled = try {
                    api.poll(a.id, a.secret.verifier, POLL_WAIT_SECONDS)
                } catch (e: CancellationException) {
                    throw e
                } catch (e: ApiException) {
                    when {
                        isUnknown(e) -> return@launch end(a, AttemptState.EXPIRED)
                        isEnded(e) -> return@launch end(a, endedState(e))
                        e.status == HttpStatusCode.TooManyRequests || e.status.value >= 500 -> {
                            if (!a.live()) return@launch end(a, AttemptState.EXPIRED)
                            // Retry-After, but never past the attempt's own end (and at most a minute).
                            val wait = e.fields.retryAfterSeconds?.times(1_000) ?: retryDelayMs(failures++)
                            delay(wait.coerceIn(1_000L, minOf(60_000L, a.msLeft().coerceAtLeast(1_000L))))
                            continue
                        }
                        else -> return@launch end(a, null)
                    }
                } catch (e: Exception) {
                    // Network (incl. a read timeout) or an unreadable answer: the attempt is kept.
                    if (!a.live()) return@launch end(a, AttemptState.EXPIRED)
                    delay(retryDelayMs(failures++))
                    continue
                }
                failures = 0
                if (current !== a) return@launch
                a.update(polled)
                when (polled.state) {
                    AttemptState.OPEN, AttemptState.CLAIMED -> refreshQr(a, polled.qr?.payload)
                    AttemptState.APPROVED -> return@launch collect(a)
                    AttemptState.DENIED, AttemptState.REPORTED, AttemptState.CANCELLED, AttemptState.EXPIRED ->
                        return@launch end(a, polled.state)
                    AttemptState.CONSUMED, AttemptState.UNKNOWN -> return@launch end(a, null)
                }
            }
        }
    }

    /** A poll in `open` may carry a new token: same attempt, new image. */
    private fun refreshQr(a: Attempt, payload: String?) {
        _state.update { st ->
            val qr = st.screen as? LoginAttemptUi.ShowQr ?: return@update st
            if (current !== a) return@update st
            st.copy(
                screen = qr.copy(
                    payload = payload ?: qr.payload,
                    minutesLeft = a.minutesLeft(),
                    totpAvailable = a.via != AttemptVia.OTHER_PHONE && a.totpAvailable,
                ),
            )
        }
    }

    /** `approved` → `collect`, exactly once per attempt; a lost answer is asked again. */
    private suspend fun collect(a: Attempt) {
        if (a.collecting) return
        a.collecting = true
        var tries = 0
        var result: Result<User>
        while (true) {
            result = authRepository.completeAttempt(a.serverUrl, a.password) { api.collect(a.id, a.secret.verifier) }
            val e = result.exceptionOrNull()
            if (e !is IOException || ++tries >= COLLECT_TRIES || current !== a) break
            delay(retryDelayMs(tries - 1))
        }
        finish(a, result, totp = false)
    }

    private fun finish(a: Attempt, result: Result<User>, totp: Boolean) {
        if (current !== a) return
        val e = result.exceptionOrNull()
        when {
            e == null -> {
                current = null
                _state.update { it.copy(screen = null, busy = false, signedIn = true) }
            }
            e is PasswordChangeRequiredException -> {
                current = null
                _state.update { it.copy(screen = null, busy = false, passwordChangeRequired = true) }
            }
            e is ApiException && isUnknown(e) -> end(a, AttemptState.EXPIRED)
            e is ApiException && isEnded(e) -> end(a, endedState(e))
            !totp && e is ApiException && e.normalizedCode == "LOGIN_ATTEMPT_NOT_READY" -> {
                a.collecting = false
                startPolling(a)
            }
            totp -> _state.update { it.copy(screen = LoginAttemptUi.Totp(error = totpError(e)), busy = false) }
            else -> end(a, null)
        }
    }

    @StringRes
    private fun totpError(e: Throwable): Int = when {
        e is ApiException && e.normalizedCode == "INVALID_MFA" -> R.string.auth_login_mfa_invalid
        e is ApiException && e.normalizedCode == "TOTP_UNAVAILABLE" -> R.string.auth_totp_unavailable
        else -> AuthErrorMapper.login(e)
    }

    private fun end(a: Attempt, state: AttemptState?) {
        if (current !== a) return
        current = null
        _state.update { it.copy(screen = LoginAttemptUi.Ended(endedText(state), a.via), busy = false) }
        pollJob?.cancel()
    }

    /** Stops the attempt on screen and cancels it on the server (best effort, bounded). */
    private fun dropAttempt() {
        pollJob?.cancel()
        stepJob?.cancel()
        pendingStart = null
        // An abandoned SSO start is over too: its late callback starts nothing.
        oidcSecret = null
        val a = current ?: return
        current = null
        appScope.launch {
            withTimeoutOrNull(CANCEL_TIMEOUT_MS) {
                try {
                    api.cancel(a.id, a.secret.verifier)
                } catch (e: CancellationException) {
                    throw e
                } catch (_: Exception) {
                    // The attempt also ends by itself (≤ 10 min).
                }
            }
        }
    }

    private fun isUnknown(e: ApiException) =
        e.normalizedCode == "LOGIN_ATTEMPT_UNKNOWN" || (e.code == null && e.status == HttpStatusCode.NotFound)

    private fun isEnded(e: ApiException) =
        e.normalizedCode == "LOGIN_ATTEMPT_ENDED" || (e.code == null && e.status == HttpStatusCode.Gone)

    private fun endedState(e: ApiException): AttemptState? =
        AttemptState.entries.firstOrNull { it.wire == e.fields.state }

    @StringRes
    private fun endedText(state: AttemptState?): Int = when (state) {
        AttemptState.DENIED -> R.string.auth_phone_ended_denied
        AttemptState.REPORTED -> R.string.auth_phone_ended_reported
        AttemptState.CANCELLED -> R.string.auth_phone_ended_cancelled
        AttemptState.EXPIRED -> R.string.auth_phone_ended_expired
        else -> R.string.auth_phone_ended_failed
    }
}
