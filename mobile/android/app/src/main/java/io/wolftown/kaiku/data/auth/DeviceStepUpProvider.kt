package io.wolftown.kaiku.data.auth

import android.app.Activity
import androidx.fragment.app.FragmentActivity
import io.wolftown.kaiku.approval.StepUpOutcome
import io.wolftown.kaiku.domain.model.StepUpAction
import io.wolftown.kaiku.domain.model.StepUpGrant
import io.wolftown.kaiku.approval.DeviceStepUp as KeyDeviceStepUp

/** A device step-up that gave no grant; [outcome] says why. */
class StepUpFailure(val outcome: StepUpOutcome) : Exception("step-up failed: $outcome")

/**
 * The key device's implementation of LP's [StepUpProvider] seam, bound with LP's
 * [DeviceStepUp] qualifier (`di/ApprovalModule`). DC1: on the key phone, in the
 * key's own session, the grant (e.g. `passkey.add` for the first passkey) comes
 * from a `device_stepup` signature. On a phone that is not the key, or when the
 * server says the session is not the key's own (`KEY_DEVICE_REQUIRED`), it falls
 * back to [fallback], LP's passkey step-up (plan B6: "callers fall back to LP's
 * Credential Manager step-up").
 *
 * Failures reach LP's `PasskeysViewModel`, so the two that LP has words for are
 * LP's types: a cancelled screen-lock prompt is [PasskeyFailure.Cancelled] (no
 * error shown) and a probation session is `FULL_SESSION_REQUIRED`. Everything
 * else is a [StepUpFailure] with its outcome. `action` is the wire string; the
 * activity must be the FragmentActivity host (L2-A2).
 */
class DeviceStepUpProvider(
    private val stepUp: KeyDeviceStepUp,
    private val fallback: StepUpProvider? = null,
) : StepUpProvider {
    override suspend fun stepUp(activity: Activity, action: String, target: String): Result<StepUpGrant> {
        val a = StepUpAction.fromWire(action) ?: return Result.failure(StepUpFailure(StepUpOutcome.Failed("UNKNOWN_ACTION")))
        val host = activity as? FragmentActivity
            ?: return Result.failure(StepUpFailure(StepUpOutcome.Failed("NO_FRAGMENT_ACTIVITY")))
        return when (val out = stepUp.stepUp(host, a, target, "")) {
            is StepUpOutcome.Granted -> Result.success(out.grant)
            StepUpOutcome.NotKeyDevice -> fallback?.stepUp(activity, action, target) ?: Result.failure(StepUpFailure(out))
            StepUpOutcome.Cancelled -> Result.failure(PasskeyFailure.Cancelled)
            StepUpOutcome.ProbationSession -> Result.failure(PasskeyFailure.Server("FULL_SESSION_REQUIRED"))
            is StepUpOutcome.Failed -> Result.failure(StepUpFailure(out))
        }
    }
}
