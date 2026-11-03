package io.wolftown.kaiku.ui.security

import androidx.annotation.StringRes
import androidx.fragment.app.FragmentActivity
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import dagger.hilt.android.lifecycle.HiltViewModel
import io.wolftown.kaiku.R
import io.wolftown.kaiku.approval.StepUpOutcome
import io.wolftown.kaiku.data.api.ApiException
import io.wolftown.kaiku.data.api.SecurityApi
import io.wolftown.kaiku.data.auth.AccountStepUp
import io.wolftown.kaiku.domain.model.AccountRequestSummary
import io.wolftown.kaiku.domain.model.Part2Factor
import io.wolftown.kaiku.domain.model.StepUpAction
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import java.io.IOException
import javax.inject.Inject
import kotlin.coroutines.cancellation.CancellationException

/**
 * L2-A10: the full-screen prompt while the account has a `waiting` or `ready`
 * request (C27 prompt `open_request`), with Cancel (a step-up with the key or a
 * passkey, or part 2 when the account has no strong authenticator) and
 * "That's right". The state belongs to one account: [refresh] runs at every
 * foreground and every session change, and another account (or a sign-out)
 * starts from nothing, so nothing of one user survives into the next.
 */
@HiltViewModel
class OpenRequestViewModel @Inject constructor(
    private val securityApi: SecurityApi,
    private val stepUp: AccountStepUp,
) : ViewModel() {

    private val _pending = MutableStateFlow<AccountRequestSummary?>(null)
    val pending: StateFlow<AccountRequestSummary?> = _pending.asStateFlow()

    private val _message = MutableStateFlow<Int?>(null)
    @get:StringRes
    val message: StateFlow<Int?> = _message.asStateFlow()

    private val _working = MutableStateFlow(false)
    val working: StateFlow<Boolean> = _working.asStateFlow()

    /** The account the state belongs to (null: signed out). */
    private var account: String? = null
    /** Bumped on every account change: an answer for the old account is dropped. */
    private var generation = 0
    private var dismissed = mutableSetOf<String>()
    private var loadJob: Job? = null

    /**
     * Reads `/api/me/security` for [userId], the signed-in account (null when
     * signed out, which only resets). Called on every foreground and session change.
     */
    fun refresh(userId: String?) {
        if (userId != account) reset(userId)
        if (userId == null || _working.value) return
        val gen = generation
        loadJob?.cancel()
        loadJob = viewModelScope.launch {
            try {
                val r = securityApi.meSecurity().openRequest
                if (gen != generation || _working.value) return@launch
                _pending.value = r?.takeIf {
                    (it.state == "waiting" || it.state == "ready") && it.subject.userId == userId && it.id !in dismissed
                }
            } catch (c: CancellationException) {
                throw c
            } catch (_: Exception) {
                // Read again at the next foreground.
            }
        }
    }

    private fun reset(userId: String?) {
        generation++
        loadJob?.cancel()
        account = userId
        dismissed = mutableSetOf()
        _pending.value = null
        _message.value = null
        _working.value = false
    }

    /** Cancel this request: a step-up for `request.cancel`/id, or [part2] when the server asks for it. */
    fun cancel(activity: FragmentActivity, part2: Part2Factor? = null) {
        val r = _pending.value ?: return
        if (_working.value) return
        _working.value = true
        _message.value = null
        val gen = generation
        viewModelScope.launch {
            var cancelled = false
            try {
                if (part2 == null) {
                    when (stepUp.stepUp(activity, StepUpAction.REQUEST_CANCEL, r.id)) {
                        is StepUpOutcome.Granted -> Unit
                        StepUpOutcome.Cancelled -> return@launch
                        StepUpOutcome.ProbationSession -> return@launch fail(R.string.security_probation_read_only)
                        StepUpOutcome.NotKeyDevice -> return@launch fail(R.string.security_step_up_none)
                        is StepUpOutcome.Failed -> return@launch fail(R.string.security_sign_in_failed)
                    }
                }
                securityApi.cancelRequest(r.id, part2)
                cancelled = true
            } catch (c: CancellationException) {
                throw c
            } catch (e: ApiException) {
                fail(cancelErrorText(e))
            } catch (_: IOException) {
                fail(R.string.security_network_error)
            } catch (_: Exception) {
                fail(R.string.security_sign_in_failed)
            } finally {
                if (gen == generation) {
                    // `working` first: the prompt closes only once nothing runs.
                    _working.value = false
                    if (cancelled) _pending.value = null
                }
            }
        }
    }

    private fun fail(@StringRes msg: Int) {
        _message.value = msg
    }

    /** "That's right" (or Back): not shown again in this run of the app. */
    fun dismiss() {
        if (_working.value) return
        _pending.value?.let { dismissed += it.id }
        _pending.value = null
    }
}

/** The cancel's refusals in the words of the notice actions (NoticeActionsViewModel). */
@StringRes
internal fun cancelErrorText(e: ApiException): Int = when (e.normalizedCode) {
    "ACCOUNT_REQUEST_COMPLETED" -> R.string.security_request_too_late
    "INVALID_MFA", "REQUEST_FACTOR_INVALID" -> R.string.security_part2_wrong
    "CANCEL_NOT_ALLOWED" -> R.string.security_action_not_allowed
    "FULL_SESSION_REQUIRED" -> R.string.security_probation_read_only
    "KEY_DEVICE_REQUIRED" -> R.string.issuer_key_phone_required
    else -> R.string.security_sign_in_failed
}

/**
 * Where the open-request prompt may cover the screen: any signed-in screen
 * (home, a chat, settings, …), never a sign-in or setup screen and never a
 * running ceremony (scanning, approving, adding a phone, "Confirm it's you"),
 * which it would interrupt; the prompt follows once that ceremony ends.
 */
object OpenRequestRules {
    fun showOver(route: String?, signInRoutes: Set<String>, ceremonyRoutes: Set<String>, promptRoute: String): Boolean =
        route != null && route != promptRoute && route !in signInRoutes && route !in ceremonyRoutes
}
