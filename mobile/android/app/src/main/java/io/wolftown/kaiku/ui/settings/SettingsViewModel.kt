package io.wolftown.kaiku.ui.settings

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import androidx.annotation.StringRes
import dagger.hilt.android.lifecycle.HiltViewModel
import io.wolftown.kaiku.R
import io.wolftown.kaiku.data.local.TokenStorage
import io.wolftown.kaiku.data.repository.AuthRepository
import io.wolftown.kaiku.domain.model.User
import io.wolftown.kaiku.domain.model.UserStatus
import io.wolftown.kaiku.push.PushPlatform
import io.wolftown.kaiku.push.PushPreferences
import io.wolftown.kaiku.push.PushRegistrar
import io.wolftown.kaiku.push.PushStatus
import io.wolftown.kaiku.push.TransportKind
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.coroutines.launch
import java.util.logging.Level
import java.util.logging.Logger
import kotlin.coroutines.cancellation.CancellationException
import javax.inject.Inject

/** What the notification section shows (and hides). */
data class NotificationSettingsState(
    val status: PushStatus = PushStatus.Unknown,
    val preferUnifiedPush: Boolean = false,
    val distributors: List<String> = emptyList(),
    val gmsAvailable: Boolean = false,
    val selectedDistributor: String? = null,
) {
    /** Without Google services UnifiedPush is the only choice, so there is nothing to switch. */
    val showUnifiedPushSwitch: Boolean get() = gmsAvailable && distributors.isNotEmpty()

    /** Only while UnifiedPush is chosen, in use, or the only transport. */
    val showDistributorPicker: Boolean
        get() = distributors.size > 1 &&
            (preferUnifiedPush || !gmsAvailable ||
                (status as? PushStatus.Active)?.kind == TransportKind.UNIFIED_PUSH)
}

/** Label for a presence status in the profile (C5 `UNKNOWN` included). */
@StringRes
internal fun statusLabelRes(status: UserStatus): Int = when (status) {
    UserStatus.ONLINE -> R.string.home_status_online
    UserStatus.IDLE -> R.string.home_status_idle
    UserStatus.DND -> R.string.home_status_dnd
    UserStatus.OFFLINE -> R.string.settings_status_offline
    UserStatus.UNKNOWN -> R.string.settings_status_unknown
}

@HiltViewModel
class SettingsViewModel @Inject constructor(
    private val authRepository: AuthRepository,
    private val tokenStorage: TokenStorage,
    private val pushRegistrar: PushRegistrar,
    private val pushPlatform: PushPlatform,
    private val pushPreferences: PushPreferences,
) : ViewModel() {

    companion object {
        private val logger = Logger.getLogger("SettingsViewModel")
    }

    private val _user = MutableStateFlow<User?>(null)
    val user: StateFlow<User?> = _user.asStateFlow()

    private val _isLoading = MutableStateFlow(false)
    val isLoading: StateFlow<Boolean> = _isLoading.asStateFlow()

    private val _error = MutableStateFlow<Int?>(null)

    /** `@StringRes` of the profile-load failure, or null. Exception text is never shown. */
    val error: StateFlow<Int?> = _error.asStateFlow()

    private val pushInputs = MutableStateFlow(readPushInputs())

    /** Push status plus the UnifiedPush choices, for the notification section. */
    val notificationState: StateFlow<NotificationSettingsState> =
        combine(pushRegistrar.status, pushInputs) { status, inputs ->
            NotificationSettingsState(
                status = status,
                preferUnifiedPush = inputs.preferUnifiedPush,
                distributors = inputs.distributors,
                gmsAvailable = inputs.gmsAvailable,
                selectedDistributor = inputs.selectedDistributor,
            )
        }.stateIn(
            viewModelScope,
            SharingStarted.Eagerly,
            NotificationSettingsState(
                status = pushRegistrar.status.value,
                preferUnifiedPush = pushInputs.value.preferUnifiedPush,
                distributors = pushInputs.value.distributors,
                gmsAvailable = pushInputs.value.gmsAvailable,
                selectedDistributor = pushInputs.value.selectedDistributor,
            ),
        )

    val serverUrl: String? get() = tokenStorage.getServerUrl()

    init {
        loadUser()
    }

    private fun loadUser() {
        viewModelScope.launch {
            _isLoading.value = true
            try {
                val result = authRepository.getCurrentUser()
                if (result.isSuccess) {
                    _user.value = result.getOrNull()
                } else {
                    _error.value = R.string.settings_profile_load_failed
                }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                logger.log(Level.WARNING, "Failed to load user", e)
                _error.value = R.string.settings_profile_load_failed
            } finally {
                _isLoading.value = false
            }
        }
    }

    /** Switch "Use UnifiedPush instead of Google": stores the choice and registers again. */
    fun setPreferUnifiedPush(enabled: Boolean) {
        pushPreferences.preferUnifiedPush = enabled
        pushInputs.value = readPushInputs()
        refreshPush()
    }

    /** The user picked a distributor app. */
    fun pickDistributor(distributor: String) {
        pushPreferences.distributor = distributor
        pushInputs.value = readPushInputs()
        refreshPush()
    }

    /** Re-reads installed distributors (e.g. after installing ntfy); registers again unless push is already on. */
    fun reloadPushState() {
        pushInputs.value = readPushInputs()
        val saved = pushPreferences.distributor
        val savedGone = saved != null && saved !in pushInputs.value.distributors
        if (savedGone || pushRegistrar.status.value !is PushStatus.Active) refreshPush()
    }

    /** Runs on the registrar's own scope, so leaving Settings does not cancel the pass. */
    private fun refreshPush() {
        pushRegistrar.refreshAsync()
    }

    private fun readPushInputs() = PushInputs(
        preferUnifiedPush = pushPreferences.preferUnifiedPush,
        distributors = pushPlatform.distributors(),
        gmsAvailable = pushPlatform.gmsAvailable(),
        selectedDistributor = pushPreferences.distributor,
    )

    private data class PushInputs(
        val preferUnifiedPush: Boolean,
        val distributors: List<String>,
        val gmsAvailable: Boolean,
        val selectedDistributor: String?,
    )

    private val _logoutComplete = Channel<Unit>(Channel.CONFLATED)
    val logoutComplete = _logoutComplete.receiveAsFlow()

    fun logout() {
        viewModelScope.launch {
            try {
                authRepository.logout()
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                logger.log(Level.WARNING, "Logout failed", e)
            }
            _logoutComplete.send(Unit)
        }
    }
}
