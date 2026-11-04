package io.wolftown.kaiku.ui.admin

import androidx.annotation.StringRes
import androidx.fragment.app.FragmentActivity
import androidx.lifecycle.SavedStateHandle
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import dagger.hilt.android.lifecycle.HiltViewModel
import io.wolftown.kaiku.R
import io.wolftown.kaiku.approval.StepUpOutcome
import io.wolftown.kaiku.data.api.AdminApi
import io.wolftown.kaiku.data.auth.AccountStepUp
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.domain.model.AdminUserDetail
import io.wolftown.kaiku.domain.model.AuthenticatorRef
import io.wolftown.kaiku.domain.model.StepUpAction
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import javax.inject.Inject
import kotlin.coroutines.cancellation.CancellationException

data class AdminUserUi(
    val user: AdminUserDetail? = null,
    val working: Boolean = false,
    @StringRes val message: Int? = null,
    /** Never for oneself (`ENROLLMENT_TARGET_SELF`): own phones come from "Add a phone". */
    val canSetUpPhone: Boolean = false,
) {
    /** "Remove device now" never targets another admin (F6 rev 4; the server refuses it too). */
    val canRemoveDevices: Boolean get() = user != null && !user.isAdmin
}

/**
 * One account in the admin area: devices with "Remove device now", `may_approve`, the open request,
 * "Set up a phone". Its step-ups take the key or a passkey ([AccountStepUp]); issuing a setup code
 * (AdminSetupPhoneScreen) stays on the admin's own key phone. One action runs at a time.
 */
@HiltViewModel
class AdminUserViewModel internal constructor(
    private val adminApi: AdminApi,
    private val stepUp: AccountStepUp,
    private val userId: String,
    private val myUserId: () -> String?,
) : ViewModel() {

    @Inject
    constructor(adminApi: AdminApi, stepUp: AccountStepUp, savedStateHandle: SavedStateHandle, authState: AuthState) :
        this(adminApi, stepUp, savedStateHandle.get<String>("userId")!!, { authState.currentUserId.value })

    private val _ui = MutableStateFlow(AdminUserUi())
    val ui: StateFlow<AdminUserUi> = _ui.asStateFlow()

    fun load() {
        viewModelScope.launch {
            try {
                val u = adminApi.user(userId)
                // A reload never clears a running action or its message.
                _ui.update { it.copy(user = u, canSetUpPhone = myUserId().let { me -> me != null && u.id != me }) }
            } catch (c: CancellationException) {
                throw c
            } catch (e: Exception) {
                _ui.update { it.copy(message = adminErrorFor(e)) }
            }
        }
    }

    fun removeNow(activity: FragmentActivity, device: AuthenticatorRef) =
        guarded(activity, StepUpAction.ADMIN_AUTHENTICATOR_REMOVE, device.id) {
            adminApi.removeAuthenticator(userId, device.id)
        }

    fun setMayApprove(activity: FragmentActivity, value: Boolean) =
        guarded(activity, StepUpAction.ADMIN_MAY_APPROVE, userId) {
            adminApi.setMayApprove(userId, value)
        }

    private fun guarded(activity: FragmentActivity, action: StepUpAction, target: String, block: suspend () -> Unit) {
        if (_ui.value.working) return
        _ui.update { it.copy(working = true, message = null) }
        viewModelScope.launch {
            when (stepUp.stepUp(activity, action, target)) {
                is StepUpOutcome.Granted -> try {
                    block()
                    _ui.update { it.copy(working = false) }
                    load()
                } catch (c: CancellationException) {
                    throw c
                } catch (e: Exception) {
                    _ui.update { it.copy(working = false, message = adminErrorFor(e)) }
                }
                StepUpOutcome.Cancelled -> _ui.update { it.copy(working = false) }
                StepUpOutcome.NotKeyDevice -> _ui.update { it.copy(working = false, message = R.string.security_step_up_none) }
                StepUpOutcome.ProbationSession -> _ui.update { it.copy(working = false, message = R.string.security_probation_read_only) }
                is StepUpOutcome.Failed -> _ui.update { it.copy(working = false, message = R.string.security_sign_in_failed) }
            }
        }
    }
}
