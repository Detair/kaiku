package io.wolftown.kaiku.ui.auth

import android.app.Activity
import androidx.annotation.StringRes
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import dagger.hilt.android.lifecycle.HiltViewModel
import io.wolftown.kaiku.data.api.AuthApi
import io.wolftown.kaiku.data.api.OidcProvider
import io.wolftown.kaiku.data.api.ProbeResult
import io.wolftown.kaiku.data.api.ServerProbe
import io.wolftown.kaiku.data.auth.PasskeyAuth
import io.wolftown.kaiku.data.auth.PasskeyFailure
import io.wolftown.kaiku.R
import io.wolftown.kaiku.data.local.TokenStorage
import io.wolftown.kaiku.data.repository.AuthRepository
import kotlin.coroutines.cancellation.CancellationException
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import javax.inject.Inject

data class LoginUiState(
    val username: String = "",
    val password: String = "",
    val isLoading: Boolean = false,
    /** `@StringRes` of the current error, or null. Server message text is never shown. */
    @StringRes val errorRes: Int? = null,
    val loginSuccess: Boolean = false,
    /** Host of the server this sign-in goes to, for display; empty when unknown. */
    val serverHost: String = "",
    /** True only when the server's registration policy is `open`; Register is hidden otherwise. */
    val registrationOpen: Boolean = false,
    /** The registration check could not reach the server; the screen offers "Try again". */
    val registrationCheckFailed: Boolean = false,
    /** The person asked to change the server; the screen navigates to the server screen. */
    val changeServerRequested: Boolean = false,
    /** `@StringRes` of the passkey sign-in error, or null. */
    @StringRes val passkeyErrorRes: Int? = null,
    /** `PASSKEY_PENDING`: the passkey is usable from this time (never signalled unknown). */
    val passkeyReadyAt: java.time.Instant? = null,
    /** The server's single sign-on providers (empty when it has none or could not say). */
    val oidcProviders: List<OidcProvider> = emptyList(),
    /** A passkey sign-in runs (its button's busy label); [isLoading] is true for any sign-in here. */
    val passkeyLoading: Boolean = false,
) {
    // Never print the password (logs, crash reports).
    override fun toString(): String =
        "LoginUiState(username=$username, isLoading=$isLoading, errorRes=$errorRes, " +
            "loginSuccess=$loginSuccess, serverHost=$serverHost, " +
            "registrationOpen=$registrationOpen)"
}

/**
 * The login form (Task B14 order): passkey (LP), "Sign in with your other
 * phone", username and password, SSO, the setup code, then recovery and
 * "Secure my account" on the server's web pages. The password, phone and SSO
 * sign-ins themselves run as C27 attempts in [LoginAttemptViewModel], which
 * holds their verifiers; this ViewModel keeps the form, LP's passkey sign-in,
 * the server and what it offers.
 */
@HiltViewModel
class LoginViewModel @Inject constructor(
    private val authRepository: AuthRepository,
    private val authApi: AuthApi,
    private val tokenStorage: TokenStorage,
    private val serverProbe: ServerProbe,
    private val passkeyAuth: PasskeyAuth,
) : ViewModel() {

    private val _uiState = MutableStateFlow(LoginUiState(serverHost = currentServerHost()))
    private var probeJob: Job? = null
    private var loginJob: Job? = null
    val uiState: StateFlow<LoginUiState> = _uiState.asStateFlow()

    init {
        probeRegistration()
        loadOidcProviders()
    }

    private fun currentServerHost(): String =
        tokenStorage.getServerUrl()?.removePrefix("https://")?.removeSuffix("/") ?: ""

    /** Asks the server whether Register may be shown; any failure leaves it hidden. */
    private fun probeRegistration() {
        val server = tokenStorage.getServerUrl() ?: return
        probeJob?.cancel()
        _uiState.update { it.copy(registrationCheckFailed = false) }
        probeJob = viewModelScope.launch {
            val result = serverProbe.probe(server)
            _uiState.update {
                it.copy(
                    registrationOpen = result is ProbeResult.Ok && result.registrationOpen,
                    registrationCheckFailed = result !is ProbeResult.Ok
                )
            }
        }
    }

    /** The SSO buttons; a server that cannot say shows none (password and phone still work). */
    private fun loadOidcProviders() {
        if (tokenStorage.getServerUrl() == null) return
        viewModelScope.launch {
            val providers = try {
                authApi.getOidcProviders()
            } catch (e: CancellationException) {
                throw e
            } catch (_: Exception) {
                emptyList()
            }
            _uiState.update { it.copy(oidcProviders = providers) }
        }
    }

    /**
     * A page of this server's web client, opened in a Custom Tab: `/recover`
     * ("I lost my phone / forgot my password") and `/secure-account`
     * (coordinator call 2). Null without a configured server.
     */
    fun serverPageUrl(path: String): String? =
        tokenStorage.getServerUrl()?.trimEnd('/')?.plus(path)

    /** "Try again" after the registration check failed (e.g. first launch offline). */
    fun onRetryRegistrationCheck() {
        probeRegistration()
    }

    /**
     * Leaves this server: drops any in-flight login and pending password change
     * (they belong to this server), clears everything typed and learned about it,
     * and asks the screen to open the server screen.
     */
    fun onChangeServerClicked() {
        probeJob?.cancel()
        loginJob?.cancel()
        authRepository.cancelPasswordChange()
        _uiState.value = LoginUiState(changeServerRequested = true)
    }

    /** Called once the screen has navigated to the server screen. */
    fun onChangeServerNavigated() {
        _uiState.update { it.copy(changeServerRequested = false) }
    }

    fun onUsernameChanged(username: String) {
        _uiState.update { it.copy(username = username) }
    }

    fun onPasswordChanged(password: String) {
        _uiState.update { it.copy(password = password) }
    }

    /** The typed password leaves the form once its sign-in is done (C4 keeps its own copy). */
    fun clearPassword() {
        _uiState.update { it.copy(password = "") }
    }

    /** A password, QR or SSO sign-in runs ([LoginAttemptViewModel]); the screen reports it. */
    @Volatile
    private var otherSignInRunning = false

    fun setOtherSignInRunning(running: Boolean) {
        otherSignInRunning = running
    }

    /**
     * Sign in with a passkey through Credential Manager (C29): a new full
     * session. The typed user name is sent as a hint, so a shared phone never
     * signs in someone else's account. Starts nothing while another sign-in
     * runs (password, SSO callback, or a passkey already: a double tap), so it
     * never cancels one; the button's `enabled` follows the same state.
     */
    fun onPasskeySignIn(activity: Activity) {
        // L2b: the password, QR and SSO sign-ins run in LoginAttemptViewModel (setOtherSignInRunning).
        if (_uiState.value.isLoading || otherSignInRunning) return
        val username = _uiState.value.username.ifBlank { null }
        _uiState.update {
            it.copy(isLoading = true, passkeyLoading = true, errorRes = null, passkeyErrorRes = null, passkeyReadyAt = null)
        }
        loginJob?.cancel()
        loginJob = viewModelScope.launch {
            passkeyAuth.loginWithPasskey(activity, username).fold(
                onSuccess = { _uiState.update { it.copy(isLoading = false, passkeyLoading = false, loginSuccess = true) } },
                onFailure = { e ->
                    _uiState.update {
                        when (e) {
                            PasskeyFailure.Cancelled -> it.copy(isLoading = false, passkeyLoading = false)
                            is PasskeyFailure.Pending -> it.copy(
                                isLoading = false,
                                passkeyLoading = false,
                                passkeyErrorRes = R.string.passkeys_error_ready_at,
                                passkeyReadyAt = e.readyAt?.let { t -> runCatching { java.time.Instant.parse(t) }.getOrNull() },
                            )
                            else -> it.copy(isLoading = false, passkeyLoading = false, passkeyErrorRes = passkeyErrorRes(e))
                        }
                    }
                },
            )
        }
    }

    fun clearError() {
        _uiState.update { it.copy(errorRes = null) }
    }
}

/** The sentence for a passkey failure (C16: never server text). */
@StringRes
fun passkeyErrorRes(e: Throwable): Int = when (e) {
    PasskeyFailure.NoCredential -> R.string.passkeys_android_none_on_device
    PasskeyFailure.NotAssociated -> R.string.passkeys_android_not_set_up_for_app
    PasskeyFailure.Removed -> R.string.passkeys_error_removed
    is PasskeyFailure.Pending -> R.string.passkeys_error_ready_at
    is PasskeyFailure.Server -> when (e.code) {
        "PASSKEY_INVALID" -> R.string.passkeys_error_invalid
        "PASSKEY_USER_MISMATCH" -> R.string.passkeys_error_user_mismatch
        "PASSKEY_CEREMONY_INVALID" -> R.string.passkeys_error_expired
        "PASSKEY_LIMIT" -> R.string.passkeys_error_limit
        "PASSKEY_ALREADY_REGISTERED" -> R.string.passkeys_error_already_registered
        "LAST_STRONG_AUTHENTICATOR" -> R.string.passkeys_error_last_strong
        "PASSKEYS_UNAVAILABLE" -> R.string.passkeys_error_unavailable
        "STEP_UP_REQUIRED" -> R.string.passkeys_error_step_up_required
        "FULL_SESSION_REQUIRED" -> R.string.passkeys_error_full_session_required
        "ACCOUNT_BANNED" -> R.string.auth_login_banned
        else -> R.string.passkeys_error_failed
    }
    else -> R.string.passkeys_error_failed
}
