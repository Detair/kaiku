package io.wolftown.kaiku.ui.auth

import android.content.Context
import android.net.Uri
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import dagger.hilt.android.lifecycle.HiltViewModel
import io.wolftown.kaiku.data.repository.AuthRepository
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
    val error: String? = null,
    val mfaRequired: Boolean = false,
    val loginSuccess: Boolean = false
)

@HiltViewModel
class LoginViewModel @Inject constructor(
    private val authRepository: AuthRepository,
    private val oidcHandler: OidcHandler
) : ViewModel() {

    private val _uiState = MutableStateFlow(LoginUiState())
    val uiState: StateFlow<LoginUiState> = _uiState.asStateFlow()

    /**
     * One-shot event flow for OIDC callback URIs received from deep links.
     * CONFLATED keeps only the latest — redelivery is harmless and trySend never blocks.
     */
    private val _oidcCallbackUri = Channel<Uri>(Channel.CONFLATED)
    val oidcCallbackUri = _oidcCallbackUri.receiveAsFlow()

    init {
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
                onFailure = { exception ->
                    _uiState.update {
                        it.copy(
                            isLoading = false,
                            error = exception.message ?: "OIDC login failed"
                        )
                    }
                }
            )
        }
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
                    if (exception is MfaRequiredException) {
                        _uiState.update {
                            it.copy(isLoading = false, mfaRequired = true)
                        }
                    } else {
                        _uiState.update {
                            it.copy(
                                isLoading = false,
                                error = exception.message ?: "Login failed"
                            )
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
        } catch (e: Exception) {
            _uiState.update {
                it.copy(error = e.message ?: "Failed to launch OIDC login")
            }
        }
    }

    fun clearError() {
        _uiState.update { it.copy(error = null) }
    }
}
