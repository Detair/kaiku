package io.wolftown.kaiku.ui.approval

import androidx.annotation.StringRes
import androidx.fragment.app.FragmentActivity
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import dagger.hilt.android.lifecycle.HiltViewModel
import io.wolftown.kaiku.R
import io.wolftown.kaiku.approval.ChallengeRejected
import io.wolftown.kaiku.approval.Decision
import io.wolftown.kaiku.approval.DeviceIdProvider
import io.wolftown.kaiku.approval.DeviceSigner
import io.wolftown.kaiku.approval.LoginContext
import io.wolftown.kaiku.approval.SigKind
import io.wolftown.kaiku.approval.SignOutcome
import io.wolftown.kaiku.approval.UvPrompt
import io.wolftown.kaiku.approval.UvResult
import io.wolftown.kaiku.data.api.ApiException
import io.wolftown.kaiku.data.api.ApprovalApi
import io.wolftown.kaiku.data.keydevice.KeyDeviceRepository
import io.wolftown.kaiku.domain.ScanPayload
import io.wolftown.kaiku.domain.model.ApprovalChallenge
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import java.io.IOException
import java.util.Base64
import java.util.logging.Logger
import javax.inject.Inject
import kotlin.coroutines.cancellation.CancellationException

sealed interface LoginApprovalUi {
    data object Claiming : LoginApprovalUi
    /** Only fields of the verified, signed context. */
    data class Context(val ctx: LoginContext) : LoginApprovalUi
    data object Working : LoginApprovalUi
    data class Done(@StringRes val msg: Int) : LoginApprovalUi
    data class Error(@StringRes val msg: Int, val rescan: Boolean) : LoginApprovalUi
}

/**
 * F2/F3 on the key device: claim the scanned token, show the verified context,
 * then "Yes, sign in" (UV, approve), "No" (unsigned deny) or "I didn't start
 * this" (UV, report). Leaving without a decision releases the claim.
 */
@HiltViewModel
class LoginApprovalViewModel @Inject constructor(
    private val approvalApi: ApprovalApi,
    private val signer: DeviceSigner,
    private val keyDevices: KeyDeviceRepository,
    private val deviceIds: DeviceIdProvider,
) : ViewModel() {

    private companion object {
        val logger: Logger = Logger.getLogger("LoginApproval")
    }

    private val _ui = MutableStateFlow<LoginApprovalUi>(LoginApprovalUi.Claiming)
    val ui: StateFlow<LoginApprovalUi> = _ui.asStateFlow()

    private var payload: ScanPayload.Login? = null
    private var challenge: ApprovalChallenge? = null
    private var context: LoginContext? = null

    /** A claimed challenge that is not decided yet (to release on leave). */
    private var open = false

    /** True once [start] ran; a recreated screen keeps its claim instead of starting again. */
    val started: Boolean get() = payload != null

    fun start(p: ScanPayload.Login) {
        payload = p
        challenge = null
        open = false
        _ui.value = LoginApprovalUi.Claiming
        val local = keyDevices.local() ?: run {
            _ui.value = LoginApprovalUi.Error(R.string.scan_not_a_key, rescan = false)
            return
        }
        viewModelScope.launch {
            val ch = try {
                approvalApi.claim(p.tokenB64u)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                _ui.value = errorFor(e)
                return@launch
            }
            challenge = ch
            open = true
            try {
                val verified = signer.check(ch, SigKind.LOGIN, local.userId, deviceIds.id())
                val ctx = verified.context as LoginContext
                context = ctx
                _ui.value = LoginApprovalUi.Context(ctx)
            } catch (e: ChallengeRejected) {
                logger.warning("Login challenge refused: ${e.reason}")
                releaseQuietly(ch.challengeId)
                _ui.value = LoginApprovalUi.Error(R.string.approval_mismatch, rescan = true)
            }
        }
    }

    fun approve(activity: FragmentActivity) = decide(activity, Decision.APPROVE)

    fun report(activity: FragmentActivity) = decide(activity, Decision.REPORT)

    private fun decide(activity: FragmentActivity, decision: Decision) {
        val ch = challenge ?: return
        val p = payload ?: return
        val ctx = context ?: return
        val local = keyDevices.local() ?: return
        if (_ui.value !is LoginApprovalUi.Context) return
        _ui.value = LoginApprovalUi.Working
        viewModelScope.launch {
            val token = try {
                Base64.getUrlDecoder().decode(p.tokenB64u)
            } catch (_: IllegalArgumentException) {
                _ui.value = LoginApprovalUi.Error(R.string.approval_mismatch, rescan = true)
                return@launch
            }
            val prompt = UvPrompt(
                R.string.security_uv_title,
                activity.getString(if (decision == Decision.APPROVE) R.string.approval_uv_approve else R.string.approval_uv_report),
            )
            when (val out = signer.sign(activity, ch, SigKind.LOGIN, local.userId, deviceIds.id(), decision, token, prompt)) {
                is SignOutcome.Rejected -> {
                    releaseQuietly(ch.challengeId)
                    _ui.value = LoginApprovalUi.Error(R.string.approval_mismatch, rescan = true)
                }
                is SignOutcome.Uv -> _ui.value = when (out.result) {
                    UvResult.Cancelled -> LoginApprovalUi.Context(ctx)
                    UvResult.LockedOut -> LoginApprovalUi.Error(R.string.security_locked_out, rescan = false)
                    UvResult.KeyInvalidated -> LoginApprovalUi.Error(R.string.security_key_invalidated, rescan = false)
                    else -> LoginApprovalUi.Error(R.string.security_sign_in_failed, rescan = true)
                }
                is SignOutcome.Signed -> try {
                    approvalApi.answer(ch.challengeId, decision.wire, p.tokenB64u, out.signatureB64u)
                    open = false
                    _ui.value = LoginApprovalUi.Done(
                        if (decision == Decision.APPROVE) R.string.approval_done else reportedText(ctx.variant),
                    )
                } catch (e: CancellationException) {
                    throw e
                } catch (e: Exception) {
                    _ui.value = errorFor(e)
                }
            }
        }
    }

    /** "No": the unsigned deny, no prompt. */
    fun deny() {
        val ch = challenge ?: return
        if (_ui.value !is LoginApprovalUi.Context) return
        _ui.value = LoginApprovalUi.Working
        viewModelScope.launch {
            try {
                approvalApi.deny(ch.challengeId)
                open = false
                _ui.value = LoginApprovalUi.Done(R.string.approval_denied)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                _ui.value = errorFor(e)
            }
        }
    }

    /** System back or leaving the screen: an undecided claim goes back to the PC. */
    fun back() {
        val ch = challenge ?: return
        if (!open) return
        open = false
        viewModelScope.launch { releaseQuietly(ch.challengeId) }
    }

    override fun onCleared() {
        back()
        super.onCleared()
    }

    private suspend fun releaseQuietly(id: String) {
        open = false
        try {
            approvalApi.release(id)
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            logger.info("Release failed: ${e.javaClass.simpleName}")
        }
    }

    private fun errorFor(e: Exception): LoginApprovalUi.Error = when (e) {
        is ApiException -> when (e.normalizedCode) {
            "LOGIN_CODE_STALE" -> LoginApprovalUi.Error(R.string.approval_code_changed, rescan = true)
            "LOGIN_ATTEMPT_CLAIMED" -> LoginApprovalUi.Error(R.string.approval_already_claimed, rescan = false)
            "LOGIN_ATTEMPT_OTHER_ACCOUNT" -> LoginApprovalUi.Error(R.string.approval_other_account, rescan = false)
            "APPROVAL_EXPIRED" -> LoginApprovalUi.Error(R.string.approval_expired, rescan = true)
            "APPROVAL_NOT_ALLOWED" -> LoginApprovalUi.Error(R.string.scan_may_approve_off, rescan = false)
            "KEY_DEVICE_REQUIRED" -> LoginApprovalUi.Error(R.string.scan_not_a_key, rescan = false)
            "APPROVAL_RESPONSE_MISMATCH", "SIGNATURE_INVALID" -> LoginApprovalUi.Error(R.string.approval_mismatch, rescan = true)
            "TOO_MANY_ATTEMPTS" -> LoginApprovalUi.Error(R.string.errors_rate_limited, rescan = false)
            else -> LoginApprovalUi.Error(R.string.security_sign_in_failed, rescan = true)
        }
        is IOException -> LoginApprovalUi.Error(R.string.security_network_error, rescan = true)
        else -> LoginApprovalUi.Error(R.string.security_sign_in_failed, rescan = true)
    }
}
