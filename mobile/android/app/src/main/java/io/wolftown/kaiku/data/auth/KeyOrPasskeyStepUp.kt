package io.wolftown.kaiku.data.auth

import androidx.fragment.app.FragmentActivity
import io.wolftown.kaiku.approval.StepUpOutcome
import io.wolftown.kaiku.data.api.ApiException
import io.wolftown.kaiku.domain.model.StepUpAction
import io.wolftown.kaiku.domain.model.StepUpGrant
import java.io.IOException
import javax.inject.Inject
import kotlin.coroutines.cancellation.CancellationException

/**
 * A step-up for an action either authenticator may confirm (F7, C26–C29 step-up
 * table): the key on the key phone, else a passkey of the account. L2's screens
 * (notice actions, the open-request prompt, Security settings, the admin user
 * view) use this; only issuing setup codes stays on the key alone
 * ([io.wolftown.kaiku.approval.DeviceStepUp], coordinator call 1).
 */
interface AccountStepUp {
    /** One grant for exactly [action] and [target]; the prompt shows only server-signed text. */
    suspend fun stepUp(activity: FragmentActivity, action: StepUpAction, target: String): StepUpOutcome
}

/** [AccountStepUp] over the composite `@DeviceStepUp` [StepUpProvider] (key, then LP's passkey fallback). */
class KeyOrPasskeyStepUp @Inject constructor(
    @DeviceStepUp private val provider: StepUpProvider,
) : AccountStepUp {
    override suspend fun stepUp(activity: FragmentActivity, action: StepUpAction, target: String): StepUpOutcome =
        provider.stepUp(activity, action.wire, target).toStepUpOutcome()
}

/**
 * The composite provider's result in L2's terms: LP's passkey failures and the
 * server's refusals become the outcomes the screens have words for.
 */
fun Result<StepUpGrant>.toStepUpOutcome(): StepUpOutcome = fold(
    onSuccess = { StepUpOutcome.Granted(it) },
    onFailure = { e ->
        when (e) {
            is CancellationException -> throw e
            is StepUpFailure -> e.outcome
            PasskeyFailure.Cancelled -> StepUpOutcome.Cancelled
            // Neither the key nor a passkey of this server on this phone.
            PasskeyFailure.NoCredential -> StepUpOutcome.NotKeyDevice
            is PasskeyFailure -> codeOutcome(e.code)
            is ApiException -> codeOutcome(e.normalizedCode ?: "HTTP_${e.status.value}")
            is IOException -> StepUpOutcome.Failed("NETWORK")
            else -> StepUpOutcome.Failed("STEP_UP_FAILED")
        }
    },
)

private fun codeOutcome(code: String): StepUpOutcome = when (code) {
    "FULL_SESSION_REQUIRED" -> StepUpOutcome.ProbationSession
    "KEY_DEVICE_REQUIRED" -> StepUpOutcome.NotKeyDevice
    else -> StepUpOutcome.Failed(code)
}
