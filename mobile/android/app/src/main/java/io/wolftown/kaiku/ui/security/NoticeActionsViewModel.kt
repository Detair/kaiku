package io.wolftown.kaiku.ui.security

import androidx.annotation.StringRes
import androidx.fragment.app.FragmentActivity
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import dagger.hilt.android.lifecycle.HiltViewModel
import io.wolftown.kaiku.R
import io.wolftown.kaiku.approval.StepUpOutcome
import io.wolftown.kaiku.data.api.AdminApi
import io.wolftown.kaiku.data.api.ApiException
import io.wolftown.kaiku.data.api.ApprovalApi
import io.wolftown.kaiku.data.api.SecurityApi
import io.wolftown.kaiku.data.auth.AccountStepUp
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.domain.model.AuthenticatorRef
import io.wolftown.kaiku.domain.model.EventAction
import io.wolftown.kaiku.domain.model.NoticeAction
import io.wolftown.kaiku.domain.model.Part2Factor
import io.wolftown.kaiku.domain.model.RepudiateResult
import io.wolftown.kaiku.domain.model.SecurityEvent
import io.wolftown.kaiku.domain.model.SecurityNotice
import io.wolftown.kaiku.domain.model.StepUpAction
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import java.io.IOException
import javax.inject.Inject
import kotlin.coroutines.cancellation.CancellationException

sealed interface NoticeActionUi {
    data object Idle : NoticeActionUi
    /** The B.6.6 warning the server attached; shown before anything happens. */
    data class Warning(@StringRes val msg: Int) : NoticeActionUi
    /** Owner rule part 2: a fresh TOTP or recovery code instead of a step-up (B.2). */
    data object NeedsPart2 : NoticeActionUi
    data object Working : NoticeActionUi
    /**
     * [removeDevice]: a cancel lost against the request's completion (409 `ACCOUNT_REQUEST_COMPLETED`);
     * this is the device the completed request added, offered as "Remove this device" (B12).
     */
    data class Result(@StringRes val msg: Int, val removeDevice: AuthenticatorRef? = null) : NoticeActionUi
    data class Error(@StringRes val msg: Int) : NoticeActionUi
}

/**
 * The usable actions of a security notice (L2-A12; as Task A9 on the web):
 * "That wasn't me" (repudiate), Cancel the request, "Remove device" (owner:
 * `key_device.remove`; admin reader: `admin.authenticator.remove`) and "I will
 * help" (contact confirmation, full session, no step-up). `via` decides
 * between a step-up and part 2; the step-up is the key or a passkey
 * ([AccountStepUp]), for exactly the event, request or device.
 */
@HiltViewModel
class NoticeActionsViewModel internal constructor(
    private val stepUp: AccountStepUp,
    private val securityApi: SecurityApi,
    private val approvalApi: ApprovalApi,
    private val adminApi: AdminApi,
    private val me: () -> String?,
) : ViewModel() {

    @Inject
    constructor(stepUp: AccountStepUp, securityApi: SecurityApi, approvalApi: ApprovalApi, adminApi: AdminApi, authState: AuthState) :
        this(stepUp, securityApi, approvalApi, adminApi, { authState.currentUserId.value })

    private val _ui = MutableStateFlow<NoticeActionUi>(NoticeActionUi.Idle)
    val ui: StateFlow<NoticeActionUi> = _ui.asStateFlow()

    private var pending: Triple<EventAction, SecurityEvent, SecurityNotice>? = null

    fun run(activity: FragmentActivity, action: EventAction, event: SecurityEvent, notice: SecurityNotice) {
        // One action at a time: a second tap never starts a second step-up.
        if (_ui.value is NoticeActionUi.Working) return
        pending = Triple(action, event, notice)
        val w = warningText(action.warning)
        if (w != null) {
            _ui.value = NoticeActionUi.Warning(w)
            return
        }
        proceed(activity)
    }

    fun confirmWarning(activity: FragmentActivity) {
        if (_ui.value is NoticeActionUi.Warning) proceed(activity)
    }

    private fun proceed(activity: FragmentActivity) {
        val (action, event, notice) = pending ?: return
        if (action.via == "part2" && (action.action == NoticeAction.REPUDIATE || action.action == NoticeAction.CANCEL_REQUEST)) {
            _ui.value = NoticeActionUi.NeedsPart2
            return
        }
        _ui.value = NoticeActionUi.Working
        viewModelScope.launch { _ui.value = execute(activity, action, event, notice, part2 = null) }
    }

    fun submitPart2(activity: FragmentActivity, factor: Part2Factor) {
        val (action, event, notice) = pending ?: return
        if (_ui.value != NoticeActionUi.NeedsPart2) return
        _ui.value = NoticeActionUi.Working
        viewModelScope.launch { _ui.value = execute(activity, action, event, notice, part2 = factor) }
    }

    fun dismiss() {
        pending = null
        _ui.value = NoticeActionUi.Idle
    }

    /**
     * "Remove this device" after a cancel came too late: the same guarded removal as the
     * notice's "Remove device" (owner `key_device.remove`, admin reader `admin.authenticator.remove`).
     */
    fun removeCompletedDevice(activity: FragmentActivity) {
        val device = (_ui.value as? NoticeActionUi.Result)?.removeDevice ?: return
        val (_, event, notice) = pending ?: return
        val remove = EventAction(NoticeAction.REMOVE_DEVICE, via = "step_up")
        pending = Triple(remove, event.copy(authenticator = device), notice)
        _ui.value = NoticeActionUi.Working
        viewModelScope.launch { _ui.value = execute(activity, remove, event.copy(authenticator = device), notice, part2 = null) }
    }

    /**
     * The device a completed request added: the one on the notice's event, else (own account) the
     * newest log entry of that request naming a device. Only key devices of one's own account are
     * offered here (an own passkey is removed in the passkey settings); an admin reader removes any.
     */
    private suspend fun completedRequestDevice(event: SecurityEvent, subjectIsMe: Boolean): AuthenticatorRef? {
        val known = event.authenticator ?: if (subjectIsMe && event.requestId != null) {
            try {
                securityApi.events(null, 50).items
                    .firstOrNull { it.requestId == event.requestId && it.authenticator != null }?.authenticator
            } catch (c: CancellationException) {
                throw c
            } catch (_: Exception) {
                null
            }
        } else {
            null
        }
        return known?.takeIf { !subjectIsMe || it.type.isEmpty() || it.type == "key_device" }
    }

    private suspend fun execute(
        activity: FragmentActivity,
        action: EventAction,
        event: SecurityEvent,
        notice: SecurityNotice,
        part2: Part2Factor?,
    ): NoticeActionUi {
        // Whose account this is decides the route (owner vs admin): never guessed from a missing id.
        val myId = me() ?: return NoticeActionUi.Error(R.string.security_sign_in_failed)
        val subjectIsMe = notice.subject.userId == myId
        val (stepAction, target) = when (action.action) {
            NoticeAction.REPUDIATE -> StepUpAction.EVENT_REPUDIATE to event.id
            NoticeAction.CANCEL_REQUEST -> StepUpAction.REQUEST_CANCEL to (event.requestId ?: return NoticeActionUi.Error(R.string.security_sign_in_failed))
            NoticeAction.REMOVE_DEVICE -> {
                val a = event.authenticator ?: return NoticeActionUi.Error(R.string.security_sign_in_failed)
                (if (subjectIsMe) StepUpAction.KEY_DEVICE_REMOVE else StepUpAction.ADMIN_AUTHENTICATOR_REMOVE) to a.id
            }
            NoticeAction.CONFIRM_CONTACT -> null to notice.subject.userId
            NoticeAction.UNKNOWN -> return NoticeActionUi.Idle
        }
        if (stepAction != null && part2 == null) {
            when (stepUp.stepUp(activity, stepAction, target)) {
                is StepUpOutcome.Granted -> Unit
                StepUpOutcome.Cancelled -> return NoticeActionUi.Idle
                StepUpOutcome.ProbationSession -> return NoticeActionUi.Error(R.string.security_probation_read_only)
                StepUpOutcome.NotKeyDevice -> return NoticeActionUi.Error(R.string.security_step_up_none)
                is StepUpOutcome.Failed -> return NoticeActionUi.Error(R.string.security_sign_in_failed)
            }
        }
        return try {
            when (action.action) {
                NoticeAction.REPUDIATE -> NoticeActionUi.Result(repudiatedText(securityApi.repudiate(event.id, part2)))
                NoticeAction.CANCEL_REQUEST -> {
                    securityApi.cancelRequest(target, part2)
                    NoticeActionUi.Result(R.string.security_request_cancelled)
                }
                NoticeAction.REMOVE_DEVICE -> {
                    if (subjectIsMe) approvalApi.removeKeyDevice(target) else adminApi.removeAuthenticator(notice.subject.userId, target)
                    NoticeActionUi.Result(R.string.security_device_removed)
                }
                NoticeAction.CONFIRM_CONTACT -> {
                    securityApi.confirmContactFor(notice.subject.userId)
                    NoticeActionUi.Result(R.string.security_contact_confirmed_done)
                }
                NoticeAction.UNKNOWN -> NoticeActionUi.Idle
            }
        } catch (c: CancellationException) {
            throw c
        } catch (e: ApiException) {
            when (e.normalizedCode) {
                "ACCOUNT_REQUEST_COMPLETED" ->
                    NoticeActionUi.Result(R.string.security_request_too_late, removeDevice = completedRequestDevice(event, subjectIsMe))
                "EVENT_ALREADY_REPUDIATED" -> NoticeActionUi.Result(R.string.security_already_repudiated)
                "REPUDIATE_NOT_ALLOWED", "CANCEL_NOT_ALLOWED" -> NoticeActionUi.Error(R.string.security_action_not_allowed)
                "INVALID_MFA", "REQUEST_FACTOR_INVALID" -> NoticeActionUi.Error(R.string.security_part2_wrong)
                "ADMIN_TARGET_IS_ADMIN" -> NoticeActionUi.Error(R.string.admin_target_is_admin)
                "LAST_STRONG_AUTHENTICATOR" -> NoticeActionUi.Error(R.string.security_last_strong)
                "FULL_SESSION_REQUIRED" -> NoticeActionUi.Error(R.string.security_probation_read_only)
                "KEY_DEVICE_REQUIRED" -> NoticeActionUi.Error(R.string.issuer_key_phone_required)
                else -> NoticeActionUi.Error(R.string.security_sign_in_failed)
            }
        } catch (_: IOException) {
            NoticeActionUi.Error(R.string.security_network_error)
        }
    }
}

@StringRes
internal fun warningText(warning: String?): Int? = when (warning) {
    "voids_own_request" -> R.string.security_warning_voids_own_request
    "voids_own_bootstrap" -> R.string.security_warning_voids_own_bootstrap
    else -> null
}

/** The first thing the person must know after "That wasn't me". */
@StringRes
internal fun repudiatedText(r: RepudiateResult): Int {
    val methods = r.event.details["auth_methods"]?.toString().orEmpty()
    return when {
        "password" in r.compromised -> R.string.security_repudiated_password
        "oidc" in methods -> R.string.security_repudiated_oidc
        else -> R.string.security_repudiated
    }
}
