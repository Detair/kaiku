package io.wolftown.kaiku.ui.admin

import androidx.annotation.StringRes
import androidx.fragment.app.FragmentActivity
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import dagger.hilt.android.lifecycle.HiltViewModel
import io.wolftown.kaiku.R
import io.wolftown.kaiku.approval.DeviceStepUp
import io.wolftown.kaiku.approval.StepUpOutcome
import io.wolftown.kaiku.data.api.AdminApi
import io.wolftown.kaiku.data.api.ApiException
import io.wolftown.kaiku.domain.model.AdminUserSummary
import io.wolftown.kaiku.domain.model.StepUpAction
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import java.io.IOException
import javax.inject.Inject
import kotlin.coroutines.cancellation.CancellationException

data class AdminUsersUi(
    val elevated: Boolean = false,
    val working: Boolean = false,
    val users: List<AdminUserSummary> = emptyList(),
    val search: String = "",
    val created: AdminUserSummary? = null,
    @StringRes val message: Int? = null,
)

/**
 * "Admin" on the phone (F5c, coordinator call 1): a device step-up for
 * `admin.elevate` opens the 15-minute window, then the user list with search
 * and "New account" (username, display name, optional recovery contact, never
 * a password).
 */
@HiltViewModel
class AdminUsersViewModel @Inject constructor(
    private val adminApi: AdminApi,
    private val stepUp: DeviceStepUp,
) : ViewModel() {

    private val _ui = MutableStateFlow(AdminUsersUi())
    val ui: StateFlow<AdminUsersUi> = _ui.asStateFlow()

    fun enter(activity: FragmentActivity) {
        if (_ui.value.working) return
        _ui.update { it.copy(working = true, message = null) }
        viewModelScope.launch {
            when (stepUp.stepUp(activity, StepUpAction.ADMIN_ELEVATE, "", activity.getString(R.string.security_step_up_admin_elevate))) {
                is StepUpOutcome.Granted -> Unit
                StepUpOutcome.Cancelled -> return@launch _ui.update { it.copy(working = false) }
                StepUpOutcome.NotKeyDevice -> return@launch _ui.update { it.copy(working = false, message = R.string.admin_key_phone_required) }
                else -> return@launch _ui.update { it.copy(working = false, message = R.string.security_sign_in_failed) }
            }
            try {
                adminApi.elevate(null)
                val users = adminApi.users(null)
                _ui.update { it.copy(working = false, elevated = true, users = users) }
            } catch (c: CancellationException) {
                throw c
            } catch (e: Exception) {
                _ui.update { it.copy(working = false, message = adminErrorFor(e)) }
            }
        }
    }

    fun search(text: String) {
        _ui.update { it.copy(search = text) }
        if (!_ui.value.elevated) return
        viewModelScope.launch {
            try {
                val users = adminApi.users(text.ifBlank { null })
                _ui.update { it.copy(users = users) }
            } catch (c: CancellationException) {
                throw c
            } catch (e: Exception) {
                _ui.update { it.copy(message = adminErrorFor(e)) }
            }
        }
    }

    /** A new, passwordless account; its phone is set up next ("Set up a phone"). */
    fun createUser(username: String, displayName: String, recoveryContactId: String?) {
        _ui.update { it.copy(working = true, message = null) }
        viewModelScope.launch {
            try {
                val u = adminApi.createUser(username.trim().lowercase(), displayName.trim().ifEmpty { username.trim() }, recoveryContactId)
                _ui.update { it.copy(working = false, created = u, users = listOf(u) + it.users.filter { x -> x.id != u.id }) }
            } catch (c: CancellationException) {
                throw c
            } catch (e: Exception) {
                _ui.update { it.copy(working = false, message = adminErrorFor(e)) }
            }
        }
    }

    fun clearCreated() = _ui.update { it.copy(created = null) }
}

@StringRes
internal fun adminErrorFor(e: Exception): Int = when (e) {
    is ApiException -> when (e.normalizedCode) {
        "ADMIN_TARGET_IS_ADMIN" -> R.string.admin_target_is_admin
        "KEY_DEVICE_REQUIRED" -> R.string.admin_key_phone_required
        "ELEVATION_REQUIRED", "STEP_UP_REQUIRED" -> R.string.admin_elevation_ended
        "USER_EXISTS", "CONFLICT" -> R.string.auth_register_username_taken
        "VALIDATION_ERROR" -> R.string.admin_invalid_input
        "ACCOUNT_REQUEST_OPEN" -> R.string.admin_request_open
        "NO_INDEPENDENT_CANCELLER" -> R.string.admin_no_canceller
        "RECOVERY_CONTACT_INVALID" -> R.string.security_contact_invalid
        else -> R.string.security_sign_in_failed
    }
    is IOException -> R.string.security_network_error
    else -> R.string.security_sign_in_failed
}
