package io.wolftown.kaiku.ui.auth

import androidx.lifecycle.ViewModel
import dagger.hilt.android.lifecycle.HiltViewModel
import io.wolftown.kaiku.data.repository.AuthRepository
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import javax.inject.Inject

@HiltViewModel
class QrRedeemViewModel @Inject constructor(
    private val authRepository: AuthRepository
) : ViewModel() {

    private val _errorRes = MutableStateFlow<Int?>(null)

    /** String resource id of the failure to show, or null. Server message text is never shown. */
    val errorRes: StateFlow<Int?> = _errorRes.asStateFlow()

    /** Redeems the QR token; returns true on success, otherwise sets [errorRes]. */
    suspend fun redeem(serverUrl: String, token: String): Boolean {
        _errorRes.value = null
        val result = authRepository.redeemQrToken(serverUrl, token)
        val failure = result.exceptionOrNull()
        if (failure != null) {
            _errorRes.value = AuthErrorMapper.qrRedeem(failure)
        }
        return failure == null
    }
}
