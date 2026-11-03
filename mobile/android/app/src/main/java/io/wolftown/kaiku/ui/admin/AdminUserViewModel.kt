package io.wolftown.kaiku.ui.admin

import androidx.annotation.StringRes
import androidx.fragment.app.FragmentActivity
import androidx.lifecycle.SavedStateHandle
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import dagger.hilt.android.lifecycle.HiltViewModel
import io.wolftown.kaiku.R
import io.wolftown.kaiku.approval.DeviceStepUp
import io.wolftown.kaiku.approval.StepUpOutcome
import io.wolftown.kaiku.data.api.AdminApi
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

/** One account in the admin area: devices with "Remove device now", `may_approve`, the open request, "Set up a phone". */
@HiltViewModel
class AdminUserViewModel internal constructor(
    private val adminApi: AdminApi,
    private val stepUp: DeviceStepUp,
    private val userId: String,
    private val myUserId: String?,
) : ViewModel() {

    @Inject
    constructor(adminApi: AdminApi, stepUp: DeviceStepUp, savedStateHandle: SavedStateHandle, authState: AuthState) :
        this(adminApi, stepUp, savedStateHandle.get<String>("userId")!!, authState.currentUserId.value)

    private val _ui = MutableStateFlow(AdminUserUi())
    val ui: StateFlow<AdminUserUi> = _ui.asStateFlow()

    fun load() {
        viewModelScope.launch {
            try {
                val u = adminApi.user(userId)
                _ui.value = AdminUserUi(user = u, canSetUpPhone = u.id != myUserId)
            } catch (c: CancellationException) {
                throw c
            } catch (e: Exception) {
                _ui.update { it.copy(message = adminErrorFor(e)) }
            }
        }
    }

    fun removeNow(activity: FragmentActivity, device: AuthenticatorRef) =
        guarded(activity, StepUpAction.ADMIN_AUTHENTICATOR_REMOVE, device.id, device.label) {
            adminApi.removeAuthenticator(userId, device.id)
        }

    fun setMayApprove(activity: FragmentActivity, value: Boolean) =
        guarded(activity, StepUpAction.ADMIN_MAY_APPROVE, userId, _ui.value.user?.displayName.orEmpty()) {
            adminApi.setMayApprove(userId, value)
        }

    private fun guarded(activity: FragmentActivity, action: StepUpAction, target: String, label: String, block: suspend () -> Unit) {
        _ui.update { it.copy(working = true, message = null) }
        viewModelScope.launch {
            when (stepUp.stepUp(activity, action, target, label)) {
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
                StepUpOutcome.NotKeyDevice -> _ui.update { it.copy(working = false, message = R.string.admin_key_phone_required) }
                else -> _ui.update { it.copy(working = false, message = R.string.security_sign_in_failed) }
            }
        }
    }
}
