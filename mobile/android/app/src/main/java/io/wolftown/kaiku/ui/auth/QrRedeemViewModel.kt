package io.wolftown.kaiku.ui.auth

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import dagger.hilt.android.lifecycle.HiltViewModel
import io.wolftown.kaiku.data.repository.AuthRepository
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import javax.inject.Inject

data class QrRedeemUiState(
    val isLoading: Boolean = true,
    val succeeded: Boolean = false,
    /** String resource id of the failure to show, or null. Server message text is never shown. */
    val errorRes: Int? = null
)

@HiltViewModel
class QrRedeemViewModel @Inject constructor(
    private val authRepository: AuthRepository
) : ViewModel() {

    private val _uiState = MutableStateFlow(QrRedeemUiState())
    val uiState: StateFlow<QrRedeemUiState> = _uiState.asStateFlow()

    private var started = false

    /**
     * Redeems the QR token once per ViewModel instance. A configuration change re-enters the
     * screen with the same instance, so the stored result is shown and the one-time token is
     * never submitted twice.
     */
    fun start(serverUrl: String, token: String) {
        if (started) return
        started = true
        viewModelScope.launch {
            val failure = authRepository.redeemQrToken(serverUrl, token).exceptionOrNull()
            _uiState.update {
                if (failure == null) {
                    it.copy(isLoading = false, succeeded = true)
                } else {
                    it.copy(isLoading = false, errorRes = AuthErrorMapper.qrRedeem(failure))
                }
            }
        }
    }
}
