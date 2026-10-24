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
import io.wolftown.kaiku.data.api.ProbeResult
import io.wolftown.kaiku.data.api.ServerProbe
import io.wolftown.kaiku.data.local.TokenStorage
import io.wolftown.kaiku.data.repository.AuthRepository
import kotlinx.coroutines.Job
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import javax.inject.Inject

data class LoginUiState(
    val username: String = "",
    val password: String = "",
    val mfaCode: String = "",
    val isLoading: Boolean = false,
    /** `@StringRes` of the current error, or null. Server message text is never shown. */
    @StringRes val error: Int? = null,
    val mfaRequired: Boolean = false,
    val loginSuccess: Boolean = false,
    /** C4: the account must choose its own password; the screen navigates to `change_password`. */
    val passwordChangeRequired: Boolean = false,
    /** Host of the server this sign-in goes to, for display; empty when unknown. */
    val serverHost: String = "",
    /** True only when the server's registration policy is `open`; Register is hidden otherwise. */
    val registrationOpen: Boolean = false,
    /** The person asked to change the server; the screen navigates to the server screen. */
    val changeServerRequested: Boolean = false
) {
    // Never print the password or MFA code (logs, crash reports).
    override fun toString(): String =
        "LoginUiState(username=$username, isLoading=$isLoading, error=$error, " +
            "mfaRequired=$mfaRequired, loginSuccess=$loginSuccess, " +
            "passwordChangeRequired=$passwordChangeRequired, serverHost=$serverHost, " +
            "registrationOpen=$registrationOpen)"
}

/** Maps a login failure to user-facing copy by status / error code (never the server message). */
@StringRes
internal fun loginErrorRes(error: Throwable): Int = when {
    error is ApiException && error.code == "INVALID_MFA" -> R.string.auth_mfa_invalid
    error is ApiException && error.status == HttpStatusCode.Unauthorized ->
        R.string.auth_invalid_credentials
    else -> R.string.auth_login_failed
}

@HiltViewModel
class LoginViewModel @Inject constructor(
    private val authRepository: AuthRepository,
    private val oidcHandler: OidcHandler,
    private val tokenStorage: TokenStorage,
    private val serverProbe: ServerProbe
) : ViewModel() {

    private val _uiState = MutableStateFlow(LoginUiState(serverHost = currentServerHost()))
    private var probeJob: Job? = null
    val uiState: StateFlow<LoginUiState> = _uiState.asStateFlow()

    /**
     * One-shot event flow for OIDC callback URIs received from deep links.
     * CONFLATED keeps only the latest — redelivery is harmless and trySend never blocks.
     */
    private val _oidcCallbackUri = Channel<Uri>(Channel.CONFLATED)
    val oidcCallbackUri = _oidcCallbackUri.receiveAsFlow()

    init {
        probeRegistration()
        // Collect OIDC callbacks and process them
        viewModelScope.launch {
            oidcCallbackUri.collect { uri ->
                handleOidcCallback(uri)
            }
        }
    }

    /**
     * Called by MainActivity when an OIDC deep link is received.
     */
    fun onOidcDeepLink(uri: Uri) {
        _oidcCallbackUri.trySend(uri)
    }

    /**
     * Handles the OIDC callback by extracting tokens and completing login.
     */
    private fun handleOidcCallback(uri: Uri) {
        _uiState.update { it.copy(isLoading = true, error = null) }

        viewModelScope.launch {
            val result = oidcHandler.handleCallback(uri, authRepository)

            result.fold(
                onSuccess = {
                    _uiState.update {
                        it.copy(isLoading = false, loginSuccess = true)
                    }
                },
                onFailure = { _ ->
                    _uiState.update {
                        it.copy(isLoading = false, error = R.string.auth_oidc_failed)
                    }
                }
            )
        }
    }

    private fun currentServerHost(): String =
        tokenStorage.getServerUrl()?.removePrefix("https://")?.removeSuffix("/") ?: ""

    /** Asks the server whether Register may be shown; any failure leaves it hidden. */
    private fun probeRegistration() {
        val server = tokenStorage.getServerUrl() ?: return
        probeJob?.cancel()
        probeJob = viewModelScope.launch {
            val result = serverProbe.probe(server)
            _uiState.update {
                it.copy(registrationOpen = result is ProbeResult.Ok && result.registrationOpen)
            }
        }
    }

    /**
     * Leaves this server: drops any in-flight login and pending password change
     * (they belong to this server), clears everything typed and learned about it,
     * and asks the screen to open the server screen.
     */
    fun onChangeServerClicked() {
        probeJob?.cancel()
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

    fun onMfaCodeChanged(code: String) {
        _uiState.update { it.copy(mfaCode = code) }
    }

    fun onLoginClicked() {
        val state = _uiState.value
        _uiState.update { it.copy(isLoading = true, error = null) }

        viewModelScope.launch {
            val mfaCode = state.mfaCode.ifBlank { null }
            val result = authRepository.login(state.username, state.password, mfaCode)

            result.fold(
                onSuccess = {
                    _uiState.update {
                        it.copy(isLoading = false, loginSuccess = true)
                    }
                },
                onFailure = { exception ->
                    when (exception) {
                        is MfaRequiredException -> _uiState.update {
                            it.copy(isLoading = false, mfaRequired = true)
                        }
                        is PasswordChangeRequiredException -> _uiState.update {
                            // The temporary password stays in AuthRepository's memory only.
                            it.copy(isLoading = false, password = "", passwordChangeRequired = true)
                        }
                        else -> _uiState.update {
                            it.copy(isLoading = false, error = loginErrorRes(exception))
                        }
                    }
                }
            )
        }
    }

    /**
     * Launches the OIDC login flow for the given provider via Chrome Custom Tabs.
     */
    fun launchOidcLogin(context: Context, providerSlug: String) {
        try {
            oidcHandler.launchOidcLogin(context, providerSlug)
        } catch (_: Exception) {
            _uiState.update { it.copy(error = R.string.auth_oidc_failed) }
        }
    }

    /** Called once the screen has navigated to `change_password`. */
    fun onPasswordChangeNavigated() {
        _uiState.update { it.copy(passwordChangeRequired = false) }
    }

    fun clearError() {
        _uiState.update { it.copy(error = null) }
    }
}
