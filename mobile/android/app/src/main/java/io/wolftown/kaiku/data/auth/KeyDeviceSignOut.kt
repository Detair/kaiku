package io.wolftown.kaiku.data.auth

import androidx.fragment.app.FragmentActivity
import io.wolftown.kaiku.approval.DeviceStepUp
import io.wolftown.kaiku.approval.StepUpOutcome
import io.wolftown.kaiku.data.api.ApiException
import io.wolftown.kaiku.data.api.ApprovalApi
import io.wolftown.kaiku.data.keydevice.KeyDeviceRepository
import io.wolftown.kaiku.data.repository.AuthRepository
import io.wolftown.kaiku.domain.model.StepUpAction
import java.io.IOException
import javax.inject.Inject
import kotlin.coroutines.cancellation.CancellationException

/**
 * The two sign-outs of a key device (B.4, C26 amendment 3): [signOut] ends the
 * session and keeps the key and the push subscription (the phone then shows
 * "Confirm it's you"); [signOutAndRemovePhone] removes this key device on the
 * server after a step-up, deletes the key, and signs out with push off.
 */
class KeyDeviceSignOut @Inject constructor(
    private val authRepository: AuthRepository,
    private val keyDevices: KeyDeviceRepository,
    private val stepUp: DeviceStepUp,
    private val approvalApi: ApprovalApi,
) {
    suspend fun signOut() = authRepository.logout()

    suspend fun signOutAndRemovePhone(activity: FragmentActivity): StepUpOutcome {
        val local = keyDevices.local() ?: return StepUpOutcome.NotKeyDevice
        val id = local.keyDeviceId ?: currentKeyDeviceId(local.userId) ?: return StepUpOutcome.Failed("NO_KEY_DEVICE_ID")
        val out = stepUp.stepUp(activity, StepUpAction.KEY_DEVICE_REMOVE, id, "")
        if (out !is StepUpOutcome.Granted) return out
        try {
            approvalApi.removeKeyDevice(id)
        } catch (e: CancellationException) {
            throw e
        } catch (e: ApiException) {
            // Already gone on the server (revoked between the step-up and the DELETE): the person's
            // wish is done; the phone forgets the key and signs out like after a removal.
            if (e.normalizedCode != "KEY_DEVICE_UNKNOWN" && e.normalizedCode != "KEY_DEVICE_REVOKED") {
                return StepUpOutcome.Failed(e.normalizedCode ?: "HTTP_${e.status.value}")
            }
        } catch (_: IOException) {
            return StepUpOutcome.Failed("NETWORK")
        } catch (_: Exception) {
            return StepUpOutcome.Failed("NETWORK")
        }
        // Forget first, so the sign-out below is a plain one and unregisters push. Push goes after the
        // removal on purpose (plan B6, C19 amendment 3): a removal that fails keeps the key and its
        // subscription, and the removal notice still reaches this phone; the server deletes the
        // subscription with the revocation, so the client's DELETE afterwards is best effort.
        keyDevices.forget()
        authRepository.logout()
        return out
    }

    /**
     * This phone's key-device id when the sign-in did not record it (`/auth/me` without
     * `session.key_device_id`): the server's list marks the current one; it is kept for next time.
     */
    private suspend fun currentKeyDeviceId(userId: String): String? = try {
        approvalApi.keyDevices().firstOrNull { it.isCurrent }?.id?.also { keyDevices.markActive(userId, it) }
    } catch (e: CancellationException) {
        throw e
    } catch (_: Exception) {
        null
    }
}
