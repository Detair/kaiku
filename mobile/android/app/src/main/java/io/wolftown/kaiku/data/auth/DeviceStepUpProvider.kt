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
 * The key device's implementation of LP's [StepUpProvider] seam (DC1: a key
 * device's `passkey.add` grant comes from here). `action` is the wire string;
 * the activity must be the FragmentActivity host (L2-A2).
 */
class DeviceStepUpProvider(private val stepUp: KeyDeviceStepUp) : StepUpProvider {
    override suspend fun stepUp(activity: Activity, action: String, target: String): Result<StepUpGrant> {
        val a = StepUpAction.fromWire(action) ?: return Result.failure(StepUpFailure(StepUpOutcome.Failed("UNKNOWN_ACTION")))
        val host = activity as? FragmentActivity
            ?: return Result.failure(StepUpFailure(StepUpOutcome.Failed("NO_FRAGMENT_ACTIVITY")))
        return when (val out = stepUp.stepUp(host, a, target, "")) {
            is StepUpOutcome.Granted -> Result.success(out.grant)
            else -> Result.failure(StepUpFailure(out))
        }
    }
}
