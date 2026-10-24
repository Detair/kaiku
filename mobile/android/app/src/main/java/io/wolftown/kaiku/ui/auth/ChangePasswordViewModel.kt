package io.wolftown.kaiku.ui.auth

import androidx.annotation.StringRes
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import dagger.hilt.android.lifecycle.HiltViewModel
import io.ktor.http.HttpStatusCode
import io.wolftown.kaiku.R
import io.wolftown.kaiku.data.api.ApiException
import io.wolftown.kaiku.data.repository.AuthRepository
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import javax.inject.Inject

data class ChangePasswordUiState(
    val newPassword: String = "",
    val confirm: String = "",
    val isLoading: Boolean = false,
    /** `@StringRes` of the current error, or null. Server message text is never shown. */
    @StringRes val error: Int? = null,
    val done: Boolean = false
) {
    override fun toString(): String =
        "ChangePasswordUiState(isLoading=$isLoading, error=$error, done=$done)"
}

/**
 * C4: an admin-created account chooses its own password on first sign-in.
 * The temporary password typed at login is `current_password`; it stays in
 * [AuthRepository]'s memory, so the user only enters the new password twice.
 */
@HiltViewModel
class ChangePasswordViewModel @Inject constructor(
    private val authRepository: AuthRepository
) : ViewModel() {

    companion object {
        /** Same bounds as the server (`UpdatePasswordRequest.new_password`). */
        const val MIN_PASSWORD_LENGTH = 8
        const val MAX_PASSWORD_LENGTH = 128
    }

    private val _uiState = MutableStateFlow(ChangePasswordUiState())
    val uiState: StateFlow<ChangePasswordUiState> = _uiState.asStateFlow()

    fun onNewPasswordChanged(value: String) {
        _uiState.update { it.copy(newPassword = value, error = null) }
    }

    fun onConfirmChanged(value: String) {
        _uiState.update { it.copy(confirm = value, error = null) }
    }

    fun onSubmit() {
        val state = _uiState.value
        if (state.isLoading || state.done) return

        val validationError = validate(state.newPassword, state.confirm)
        if (validationError != null) {
            _uiState.update { it.copy(error = validationError) }
            return
        }

        _uiState.update { it.copy(isLoading = true, error = null) }
        viewModelScope.launch {
            authRepository.completePasswordChange(state.newPassword).fold(
                onSuccess = {
                    _uiState.update {
                        it.copy(isLoading = false, newPassword = "", confirm = "", done = true)
                    }
                },
                onFailure = { e ->
                    _uiState.update { it.copy(isLoading = false, error = errorRes(e)) }
                }
            )
        }
    }

    /** Back: drop the pending change; the user returns to the login screen. */
    fun onCancel() {
        authRepository.cancelPasswordChange()
    }

    @StringRes
    private fun validate(newPassword: String, confirm: String): Int? {
        // Count code points like the server's validator, not UTF-16 units.
        val length = newPassword.codePointCount(0, newPassword.length)
        return when {
            length < MIN_PASSWORD_LENGTH -> R.string.auth_password_too_short
            length > MAX_PASSWORD_LENGTH -> R.string.auth_password_too_long
            newPassword != confirm -> R.string.auth_password_mismatch
            else -> null
        }
    }

    @StringRes
    private fun errorRes(e: Throwable): Int = when {
        // No pending change (e.g. the app was restarted): sign in again.
        e is IllegalStateException -> R.string.auth_password_change_expired
        // The only server-side validation left is "must differ from the temporary password".
        e is ApiException && e.code == "VALIDATION_ERROR" -> R.string.auth_password_must_differ
        e is ApiException && e.status == HttpStatusCode.Unauthorized ->
            R.string.auth_password_change_expired
        else -> R.string.auth_password_change_failed
    }
}
