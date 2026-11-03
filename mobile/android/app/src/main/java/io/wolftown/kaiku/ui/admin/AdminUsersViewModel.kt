package io.wolftown.kaiku.ui.admin

import androidx.annotation.StringRes
import androidx.fragment.app.FragmentActivity
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import dagger.hilt.android.lifecycle.HiltViewModel
import io.wolftown.kaiku.R
import io.wolftown.kaiku.approval.StepUpOutcome
import io.wolftown.kaiku.data.api.AdminApi
import io.wolftown.kaiku.data.api.ApiException
import io.wolftown.kaiku.data.auth.AccountStepUp
import kotlinx.coroutines.Job
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
 * "Admin" on the phone (F5c, coordinator call 1): a step-up for `admin.elevate`
 * (the key, or a passkey) opens the 15-minute window, then the user list with
 * search and "New account" (username, display name, optional recovery contact,
 * never a password). Setup codes still need the admin's own key phone.
 */
@HiltViewModel
class AdminUsersViewModel @Inject constructor(
    private val adminApi: AdminApi,
    private val stepUp: AccountStepUp,
) : ViewModel() {

    private var searchJob: Job? = null

    private val _ui = MutableStateFlow(AdminUsersUi())
    val ui: StateFlow<AdminUsersUi> = _ui.asStateFlow()

    fun enter(activity: FragmentActivity) {
        if (_ui.value.working) return
        _ui.update { it.copy(working = true, message = null) }
        viewModelScope.launch {
            when (stepUp.stepUp(activity, StepUpAction.ADMIN_ELEVATE, "")) {
                is StepUpOutcome.Granted -> Unit
                StepUpOutcome.Cancelled -> return@launch _ui.update { it.copy(working = false) }
                StepUpOutcome.NotKeyDevice -> return@launch _ui.update { it.copy(working = false, message = R.string.security_step_up_none) }
                StepUpOutcome.ProbationSession -> return@launch _ui.update { it.copy(working = false, message = R.string.security_probation_read_only) }
                is StepUpOutcome.Failed -> return@launch _ui.update { it.copy(working = false, message = R.string.security_sign_in_failed) }
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
        // Only the latest query's answer is shown: an earlier, slower one never wins.
        searchJob?.cancel()
        searchJob = viewModelScope.launch {
            try {
                val users = adminApi.users(text.ifBlank { null })
                if (_ui.value.search == text) _ui.update { it.copy(users = users) }
            } catch (c: CancellationException) {
                throw c
            } catch (e: Exception) {
                _ui.update { it.copy(message = adminErrorFor(e)) }
            }
        }
    }

    /** A new, passwordless account; its phone is set up next ("Set up a phone"). */
    fun createUser(username: String, displayName: String, recoveryContactId: String?) {
        if (_ui.value.working) return
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
        "ENROLLMENT_TARGET_SELF" -> R.string.admin_target_self
        else -> R.string.security_sign_in_failed
    }
    is IOException -> R.string.security_network_error
    else -> R.string.security_sign_in_failed
}
