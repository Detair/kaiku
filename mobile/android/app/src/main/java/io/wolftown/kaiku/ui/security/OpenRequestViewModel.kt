package io.wolftown.kaiku.ui.security

import androidx.annotation.StringRes
import androidx.fragment.app.FragmentActivity
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import dagger.hilt.android.lifecycle.HiltViewModel
import io.wolftown.kaiku.R
import io.wolftown.kaiku.approval.DeviceStepUp
import io.wolftown.kaiku.approval.StepUpOutcome
import io.wolftown.kaiku.data.api.ApiException
import io.wolftown.kaiku.data.api.SecurityApi
import io.wolftown.kaiku.domain.model.AccountRequestSummary
import io.wolftown.kaiku.domain.model.Part2Factor
import io.wolftown.kaiku.domain.model.StepUpAction
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import javax.inject.Inject
import kotlin.coroutines.cancellation.CancellationException

/**
 * L2-A10: the full-screen prompt at app open while the account has a `waiting`
 * or `ready` request (C27 prompt `open_request`), with Cancel (step-up, or
 * part 2 when the account has no strong authenticator) and "That's right".
 */
@HiltViewModel
class OpenRequestViewModel @Inject constructor(
    private val securityApi: SecurityApi,
    private val stepUp: DeviceStepUp,
) : ViewModel() {

    private val _pending = MutableStateFlow<AccountRequestSummary?>(null)
    val pending: StateFlow<AccountRequestSummary?> = _pending.asStateFlow()

    private val _message = MutableStateFlow<Int?>(null)
    @get:StringRes
    val message: StateFlow<Int?> = _message.asStateFlow()

    private val _working = MutableStateFlow(false)
    val working: StateFlow<Boolean> = _working.asStateFlow()

    private var dismissed = mutableSetOf<String>()

    fun load() {
        viewModelScope.launch {
            try {
                val r = securityApi.meSecurity().openRequest
                _pending.value = r?.takeIf { (it.state == "waiting" || it.state == "ready") && it.id !in dismissed }
            } catch (c: CancellationException) {
                throw c
            } catch (_: Exception) {
                // Shown again at the next app open.
            }
        }
    }

    /** Cancel this request: the device step-up for `request.cancel`/id, or [part2] when the server asks for it. */
    fun cancel(activity: FragmentActivity, part2: Part2Factor? = null) {
        val r = _pending.value ?: return
        _working.value = true
        viewModelScope.launch {
            try {
                if (part2 == null) {
                    when (stepUp.stepUp(activity, StepUpAction.REQUEST_CANCEL, r.id, r.subject.displayName.ifEmpty { r.subject.username })) {
                        is StepUpOutcome.Granted -> Unit
                        StepUpOutcome.Cancelled -> return@launch
                        else -> {
                            _message.value = R.string.security_sign_in_failed
                            return@launch
                        }
                    }
                }
                securityApi.cancelRequest(r.id, part2)
                _pending.value = null
            } catch (c: CancellationException) {
                throw c
            } catch (e: ApiException) {
                _message.value = if (e.normalizedCode == "ACCOUNT_REQUEST_COMPLETED") R.string.security_request_too_late else R.string.security_sign_in_failed
            } catch (_: Exception) {
                _message.value = R.string.security_network_error
            } finally {
                _working.value = false
            }
        }
    }

    /** "That's right": not shown again in this run of the app. */
    fun dismiss() {
        _pending.value?.let { dismissed += it.id }
        _pending.value = null
    }
}
