package io.wolftown.kaiku.ui.settings.security

import androidx.annotation.StringRes
import androidx.fragment.app.FragmentActivity
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import dagger.hilt.android.lifecycle.HiltViewModel
import io.wolftown.kaiku.R
import io.wolftown.kaiku.approval.DeviceStepUp
import io.wolftown.kaiku.approval.StepUpOutcome
import io.wolftown.kaiku.approval.UvClass
import io.wolftown.kaiku.data.api.ApiException
import io.wolftown.kaiku.data.api.ApprovalApi
import io.wolftown.kaiku.data.api.DmApi
import io.wolftown.kaiku.data.api.SecurityApi
import io.wolftown.kaiku.data.auth.KeyDeviceSignOut
import io.wolftown.kaiku.data.keydevice.KeyDeviceRepository
import io.wolftown.kaiku.data.keydevice.KeyDeviceState
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.domain.model.DmConversation
import io.wolftown.kaiku.domain.model.KeyDevice
import io.wolftown.kaiku.domain.model.KeyDeviceStatus
import io.wolftown.kaiku.domain.model.MeSecurity
import io.wolftown.kaiku.domain.model.SessionItem
import io.wolftown.kaiku.domain.model.StepUpAction
import io.wolftown.kaiku.ui.approval.canAddPhone
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import java.io.IOException
import javax.inject.Inject
import kotlin.coroutines.cancellation.CancellationException

/** Someone from the person's DMs who can be named recovery contact (never the system account). */
data class ContactCandidate(val userId: String, val username: String, val displayName: String)

data class SecuritySettingsUi(
    val loading: Boolean = true,
    /** Probation (B.4): everything shown, nothing changeable. */
    val readOnly: Boolean = true,
    val me: MeSecurity? = null,
    val isKeyDevice: Boolean = false,
    val thisPhoneUv: UvClass? = null,
    val keyDevices: List<KeyDevice> = emptyList(),
    val sessions: List<SessionItem> = emptyList(),
    val canAddPhone: Boolean = false,
    /** Shown once, inside SecureScreen, until "I've written them down". */
    val newRecoveryCodes: List<String>? = null,
    @StringRes val message: Int? = null,
    /** After an authenticator change (ASVS 7.4.3). */
    val offerSignOutOthers: Boolean = false,
    val working: Boolean = false,
    val signedOut: Boolean = false,
) {
    override fun toString(): String =
        "SecuritySettingsUi(readOnly=$readOnly, keyDevices=${keyDevices.size}, sessions=${sessions.size}, " +
            "newRecoveryCodes=${newRecoveryCodes?.size}, message=$message)"
}

/**
 * Security settings on the key phone (Task B11). Every change goes through the
 * device step-up for exactly its action and target; password and TOTP are
 * shown read-only (coordinator call 3: managed on web/desktop with a passkey).
 */
@HiltViewModel
class SecuritySettingsViewModel internal constructor(
    private val securityApi: SecurityApi,
    private val approvalApi: ApprovalApi,
    private val stepUp: DeviceStepUp,
    private val keyDevices: KeyDeviceRepository,
    private val signOut: KeyDeviceSignOut,
    private val dms: suspend () -> List<DmConversation>,
    private val me: () -> String?,
) : ViewModel() {

    @Inject
    constructor(
        securityApi: SecurityApi,
        approvalApi: ApprovalApi,
        stepUp: DeviceStepUp,
        keyDevices: KeyDeviceRepository,
        signOut: KeyDeviceSignOut,
        dmApi: DmApi,
        authState: AuthState,
    ) : this(securityApi, approvalApi, stepUp, keyDevices, signOut, dmApi::getDms, { authState.currentUserId.value })

    private val _ui = MutableStateFlow(SecuritySettingsUi())
    val ui: StateFlow<SecuritySettingsUi> = _ui.asStateFlow()

    fun load() {
        viewModelScope.launch {
            try {
                val m = securityApi.meSecurity()
                val devices = approvalApi.keyDevices().filter { it.state != KeyDeviceStatus.REVOKED }
                val sessions = securityApi.sessions()
                val isKey = keyDevices.current() is KeyDeviceState.Active
                _ui.update {
                    it.copy(
                        loading = false,
                        readOnly = m.session?.assurance != "full",
                        me = m,
                        isKeyDevice = isKey,
                        thisPhoneUv = keyDevices.local()?.uvClass,
                        keyDevices = devices.sortedByDescending { d -> d.isCurrent },
                        sessions = sessions,
                        canAddPhone = canAddPhone(m.mayApprove, isKey),
                    )
                }
            } catch (c: CancellationException) {
                throw c
            } catch (e: Exception) {
                _ui.update { it.copy(loading = false, message = errorFor(e)) }
            }
        }
    }

    /** Runs [block] after a step-up for ([action], [target]); probation and refusals become messages. */
    private fun guarded(
        activity: FragmentActivity,
        action: StepUpAction,
        target: String,
        label: String,
        authenticatorChange: Boolean = false,
        block: suspend () -> Unit,
    ) {
        if (_ui.value.readOnly) {
            _ui.update { it.copy(message = R.string.security_probation_read_only) }
            return
        }
        _ui.update { it.copy(working = true, message = null) }
        viewModelScope.launch {
            when (val out = stepUp.stepUp(activity, action, target, label)) {
                is StepUpOutcome.Granted -> try {
                    block()
                    _ui.update { it.copy(working = false, offerSignOutOthers = authenticatorChange || it.offerSignOutOthers) }
                    load()
                } catch (c: CancellationException) {
                    throw c
                } catch (e: Exception) {
                    _ui.update { it.copy(working = false, message = errorFor(e)) }
                }
                StepUpOutcome.Cancelled -> _ui.update { it.copy(working = false) }
                StepUpOutcome.ProbationSession -> _ui.update { it.copy(working = false, message = R.string.security_probation_read_only) }
                StepUpOutcome.NotKeyDevice -> _ui.update { it.copy(working = false, message = R.string.issuer_key_phone_required) }
                is StepUpOutcome.Failed -> _ui.update { it.copy(working = false, message = R.string.security_sign_in_failed) }
            }
        }
    }

    fun removeKeyDevice(activity: FragmentActivity, d: KeyDevice) =
        guarded(activity, StepUpAction.KEY_DEVICE_REMOVE, d.id, d.label, authenticatorChange = true) {
            approvalApi.removeKeyDevice(d.id)
        }

    /** A key device's session row is "Remove device"; any other is "End". */
    fun endSession(activity: FragmentActivity, s: SessionItem) {
        val a = s.authenticator
        if (a != null && a.type == "key_device") {
            guarded(activity, StepUpAction.KEY_DEVICE_REMOVE, a.id, a.label, authenticatorChange = true) {
                approvalApi.removeKeyDevice(a.id)
            }
        } else {
            guarded(activity, StepUpAction.SESSIONS_END, s.id, s.device) { securityApi.endSession(s.id) }
        }
    }

    fun endOtherSessions(activity: FragmentActivity) =
        guarded(activity, StepUpAction.SESSIONS_END, "others", activity.getString(R.string.security_sessions_others)) {
            securityApi.endOtherSessions()
            _ui.update { it.copy(offerSignOutOthers = false) }
        }

    fun confirmSignOutOthers(activity: FragmentActivity) {
        _ui.update { it.copy(offerSignOutOthers = false) }
        endOtherSessions(activity)
    }

    fun dismissSignOutOthers() = _ui.update { it.copy(offerSignOutOthers = false) }

    fun createRecoveryCodes(activity: FragmentActivity) =
        guarded(activity, StepUpAction.RECOVERY_CODES_CREATE, "", "") {
            val codes = securityApi.createRecoveryCodes()
            _ui.update { it.copy(newRecoveryCodes = codes) }
        }

    fun acknowledgeRecoveryCodes() = _ui.update { it.copy(newRecoveryCodes = null) }

    /** People from the DMs, never the system account or oneself. */
    suspend fun contactCandidates(): List<ContactCandidate> {
        val self = me()
        return try {
            dms().flatMap { it.participants }
                .filter { !it.isSystem && it.userId != self }
                .distinctBy { it.userId }
                .map { ContactCandidate(it.userId, it.username, it.displayName) }
        } catch (c: CancellationException) {
            throw c
        } catch (_: Exception) {
            emptyList()
        }
    }

    fun setRecoveryContact(activity: FragmentActivity, c: ContactCandidate) =
        guarded(activity, StepUpAction.RECOVERY_CONTACT_SET, c.userId, c.displayName.ifEmpty { c.username }) {
            securityApi.setRecoveryContact(c.userId)
        }

    fun removeRecoveryContact(activity: FragmentActivity) =
        guarded(activity, StepUpAction.RECOVERY_CONTACT_SET, "", "") { securityApi.removeRecoveryContact() }

    fun signOut() {
        viewModelScope.launch {
            signOut.signOut()
            _ui.update { it.copy(signedOut = true) }
        }
    }

    fun signOutAndRemovePhone(activity: FragmentActivity) {
        _ui.update { it.copy(working = true, message = null) }
        viewModelScope.launch {
            when (signOut.signOutAndRemovePhone(activity)) {
                is StepUpOutcome.Granted -> _ui.update { it.copy(working = false, signedOut = true) }
                StepUpOutcome.Cancelled -> _ui.update { it.copy(working = false) }
                else -> _ui.update { it.copy(working = false, message = R.string.security_sign_in_failed) }
            }
        }
    }

    fun clearMessage() = _ui.update { it.copy(message = null) }

    @StringRes
    private fun errorFor(e: Exception): Int = when (e) {
        is ApiException -> when (e.normalizedCode) {
            "LAST_STRONG_AUTHENTICATOR" -> R.string.security_last_strong
            "RECOVERY_CONTACT_INVALID" -> R.string.security_contact_invalid
            "FULL_SESSION_REQUIRED" -> R.string.security_probation_read_only
            "TOO_MANY_ATTEMPTS" -> R.string.errors_rate_limited
            else -> R.string.security_sign_in_failed
        }
        is IOException -> R.string.security_network_error
        else -> R.string.security_sign_in_failed
    }
}
