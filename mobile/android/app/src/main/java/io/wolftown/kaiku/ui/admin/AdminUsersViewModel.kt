package io.wolftown.kaiku.ui.admin

import androidx.annotation.StringRes
import androidx.fragment.app.FragmentActivity
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import dagger.hilt.android.lifecycle.HiltViewModel
import io.wolftown.kaiku.R
import io.wolftown.kaiku.approval.StepUpOutcome
import io.wolftown.kaiku.data.api.AdminApi
import io.wolftown.kaiku.data.api.GuildApi
import io.wolftown.kaiku.domain.model.Guild
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
    /** "New account" form is open. */
    val newAccount: Boolean = false,
    /** The admin's own servers, offered for the new account. */
    val servers: List<Guild> = emptyList(),
    /** Ticked servers (all of them when the form opens, at most [MAX_NEW_ACCOUNT_SERVERS]). */
    val selectedServers: Set<String> = emptySet(),
    val serversLoading: Boolean = false,
    val serversFailed: Boolean = false,
    /** A notice for the server rows (the limit of 10), shown next to them. */
    @StringRes val serversNotice: Int? = null,
)

/** C23: a new account joins at most this many servers at once. */
const val MAX_NEW_ACCOUNT_SERVERS = 10

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
    private val guildApi: GuildApi,
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

    /**
     * Opens "New account" and loads the admin's servers, all ticked (the first
     * [MAX_NEW_ACCOUNT_SERVERS] if there are more). If they cannot be loaded
     * the account can still be created, without a server.
     */
    fun startNewAccount() {
        // A fresh form: nothing selected for an earlier account carries over, also if the list fails.
        _ui.update { it.copy(newAccount = true, servers = emptyList(), selectedServers = emptySet(), serversNotice = null) }
        loadServers()
    }

    fun loadServers() {
        if (_ui.value.serversLoading) return
        _ui.update { it.copy(serversLoading = true, serversFailed = false) }
        viewModelScope.launch {
            try {
                val servers = guildApi.getGuilds()
                _ui.update {
                    it.copy(
                        serversLoading = false,
                        servers = servers,
                        selectedServers = servers.take(MAX_NEW_ACCOUNT_SERVERS).mapTo(LinkedHashSet()) { g -> g.id },
                    )
                }
            } catch (c: CancellationException) {
                throw c
            } catch (e: Exception) {
                _ui.update { it.copy(serversLoading = false, serversFailed = true) }
            }
        }
    }

    /** Ticks or unticks a server; an eleventh is refused with a hint. */
    fun toggleServer(guildId: String) = _ui.update {
        when {
            guildId in it.selectedServers -> it.copy(selectedServers = it.selectedServers - guildId, serversNotice = null)
            it.selectedServers.size >= MAX_NEW_ACCOUNT_SERVERS -> it.copy(serversNotice = R.string.admin_new_account_servers_max)
            else -> it.copy(selectedServers = it.selectedServers + guildId, serversNotice = null)
        }
    }

    /** A new, passwordless account on the ticked servers; its phone is set up next ("Set up a phone"). */
    fun createUser(username: String, displayName: String, recoveryContactId: String?) {
        // While the servers load, the ticks are not known yet: an account would silently get none.
        if (_ui.value.working || _ui.value.serversLoading) return
        _ui.update { it.copy(working = true, message = null) }
        val state = _ui.value
        // In the order of the server list; only servers that are still offered.
        val guildIds = state.servers.map { it.id }.filter { it in state.selectedServers }
        viewModelScope.launch {
            try {
                val u = adminApi.createUser(
                    username.trim().lowercase(),
                    displayName.trim().ifEmpty { username.trim() },
                    recoveryContactId,
                    guildIds,
                )
                _ui.update {
                    it.copy(working = false, created = u, newAccount = false, users = listOf(u) + it.users.filter { x -> x.id != u.id })
                }
            } catch (c: CancellationException) {
                throw c
            } catch (e: Exception) {
                _ui.update { it.copy(working = false, message = createErrorFor(e)) }
            }
        }
    }

    /** The server's `VALIDATION_ERROR` on create is a bad username or a ticked server that is gone or full. */
    @StringRes
    private fun createErrorFor(e: Exception): Int =
        if (e is ApiException && e.normalizedCode == "VALIDATION_ERROR") R.string.admin_create_invalid else adminErrorFor(e)

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
