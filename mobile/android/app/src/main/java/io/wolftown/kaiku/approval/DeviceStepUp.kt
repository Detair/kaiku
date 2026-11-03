package io.wolftown.kaiku.approval

import androidx.fragment.app.FragmentActivity
import io.wolftown.kaiku.R
import io.wolftown.kaiku.data.api.ApiException
import io.wolftown.kaiku.data.api.ApprovalApi
import io.wolftown.kaiku.data.api.AuthApi
import io.wolftown.kaiku.data.keydevice.KeyDeviceRepository
import io.wolftown.kaiku.data.keydevice.KeyDeviceState
import io.wolftown.kaiku.domain.model.StepUpAction
import io.wolftown.kaiku.domain.model.StepUpGrant
import java.io.IOException
import javax.inject.Inject
import kotlin.coroutines.cancellation.CancellationException

sealed interface StepUpOutcome {
    data class Granted(val grant: StepUpGrant) : StepUpOutcome
    data object Cancelled : StepUpOutcome
    /** Not this phone's key session: callers fall back to a passkey step-up when the account has one. */
    data object NotKeyDevice : StepUpOutcome
    data object ProbationSession : StepUpOutcome
    data class Failed(val code: String) : StepUpOutcome
}

/** The C26 device id of this install (the C19 push device id). */
fun interface DeviceIdProvider {
    fun id(): String
}

/**
 * F7 step-up with the key (C26 `device_stepup`): one grant per action and
 * target. The verified context must name this session's `sid` and exactly the
 * requested action and target before the UV prompt, whose subtitle says what
 * is confirmed with the server's `target_label`.
 */
class DeviceStepUp @Inject constructor(
    private val approvalApi: ApprovalApi,
    private val signer: DeviceSigner,
    private val keyDevices: KeyDeviceRepository,
    private val deviceIds: DeviceIdProvider,
    private val authApi: AuthApi,
) {
    suspend fun stepUp(activity: FragmentActivity, action: StepUpAction, target: String, targetLabel: String): StepUpOutcome {
        val local = keyDevices.local()
        if (local == null || keyDevices.current() !is KeyDeviceState.Active) return StepUpOutcome.NotKeyDevice
        return try {
            val session = authApi.getMe().session ?: return StepUpOutcome.ProbationSession
            if (session.assurance != "full") return StepUpOutcome.ProbationSession
            val ch = approvalApi.stepUp(action.wire, target)
            val verified = try {
                signer.check(ch, SigKind.DEVICE_STEPUP, local.userId, deviceIds.id())
            } catch (e: ChallengeRejected) {
                return StepUpOutcome.Failed("CHALLENGE_${e.reason}")
            }
            val ctx = verified.context as DeviceStepUpContext
            if (ctx.sid != session.id || ctx.action != action.wire || ctx.target != target) {
                return StepUpOutcome.Failed("CONTEXT_MISMATCH")
            }
            val (res, takesLabel) = StepUpText.prompt(action)
            // Only the signed label is shown (C26: display only the parsed context); [targetLabel] is not.
            val label = ctx.targetLabel
            val subtitle = if (takesLabel) activity.getString(res, label) else activity.getString(res)
            val out = signer.sign(
                activity, ch, SigKind.DEVICE_STEPUP, local.userId, deviceIds.id(), Decision.APPROVE, ByteArray(0),
                UvPrompt(R.string.security_uv_title, subtitle),
            )
            when (out) {
                is SignOutcome.Rejected -> StepUpOutcome.Failed("CHALLENGE_${out.reason}")
                is SignOutcome.Uv -> uvOutcome(out.result)
                is SignOutcome.Signed -> {
                    val answer = approvalApi.answer(ch.challengeId, Decision.APPROVE.wire, "", out.signatureB64u)
                    answer.grant?.let { StepUpOutcome.Granted(it) } ?: StepUpOutcome.Failed("NO_GRANT")
                }
            }
        } catch (e: CancellationException) {
            throw e
        } catch (e: ApiException) {
            when (e.normalizedCode) {
                "KEY_DEVICE_REQUIRED" -> StepUpOutcome.NotKeyDevice
                "FULL_SESSION_REQUIRED" -> StepUpOutcome.ProbationSession
                else -> StepUpOutcome.Failed(e.normalizedCode ?: "HTTP_${e.status.value}")
            }
        } catch (_: IOException) {
            StepUpOutcome.Failed("NETWORK")
        }
    }

    private fun uvOutcome(r: UvResult): StepUpOutcome = when (r) {
        UvResult.Cancelled -> StepUpOutcome.Cancelled
        UvResult.LockedOut -> StepUpOutcome.Failed("UV_LOCKED_OUT")
        UvResult.KeyInvalidated -> StepUpOutcome.Failed("KEY_INVALIDATED")
        is UvResult.Failed -> StepUpOutcome.Failed(r.code)
        is UvResult.Signed -> StepUpOutcome.Failed("UNEXPECTED")
    }
}
