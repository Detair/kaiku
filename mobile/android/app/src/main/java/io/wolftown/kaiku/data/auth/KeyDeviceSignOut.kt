package io.wolftown.kaiku.data.auth

import androidx.fragment.app.FragmentActivity
import io.wolftown.kaiku.approval.DeviceStepUp
import io.wolftown.kaiku.approval.StepUpOutcome
import io.wolftown.kaiku.data.api.ApiException
import io.wolftown.kaiku.data.api.ApprovalApi
import io.wolftown.kaiku.data.keydevice.Ask
import io.wolftown.kaiku.data.keydevice.Change
import io.wolftown.kaiku.data.keydevice.KeyAnswers
import io.wolftown.kaiku.data.keydevice.KeyDeviceRepository
import io.wolftown.kaiku.data.keydevice.ServerAnswer
import io.wolftown.kaiku.data.keydevice.StoredKeyDevice
import io.wolftown.kaiku.data.local.TokenStorage
import io.wolftown.kaiku.data.repository.AuthRepository
import io.wolftown.kaiku.domain.KaikuPayloads
import io.wolftown.kaiku.domain.model.StepUpAction
import io.wolftown.kaiku.domain.model.KeyDeviceStatus
import java.io.IOException
import javax.inject.Inject
import kotlin.coroutines.cancellation.CancellationException

/**
 * The sign-outs of a key device (B.4, C26 amendment 3): [signOut] ends the
 * session and keeps the key and the push subscription (the phone then shows
 * "Confirm it's you"); [signOutAndRemovePhone] removes this key device on the
 * server after a step-up, deletes the key, and signs out with push off;
 * [removeSignedOutPhone] does the same for a phone that is not signed in, without ever
 * signing the app in ([PhoneRemoval]).
 */
class KeyDeviceSignOut @Inject constructor(
    private val authRepository: AuthRepository,
    private val keyDevices: KeyDeviceRepository,
    private val stepUp: DeviceStepUp,
    private val approvalApi: ApprovalApi,
    private val answers: KeyAnswers,
    private val tokenStorage: TokenStorage,
    private val removal: PhoneRemoval,
) {

    suspend fun signOut() = authRepository.logout()

    suspend fun signOutAndRemovePhone(activity: FragmentActivity): StepUpOutcome {
        var rec = keyDevices.record.value ?: return StepUpOutcome.NotKeyDevice
        // The server this session talks to; the answer below counts only if it is the key's own.
        val asked = tokenStorage.getServerUrl()?.let(KaikuPayloads::canonicalOrigin)
        val id = rec.device.keyDeviceId
            ?: currentKeyDeviceId(rec)?.also { rec = it }?.device?.keyDeviceId
            ?: return StepUpOutcome.Failed("NO_KEY_DEVICE_ID")
        val out = stepUp.stepUp(activity, StepUpAction.KEY_DEVICE_REMOVE, id, "")
        if (out !is StepUpOutcome.Granted) return out
        val answer = try {
            val removed = approvalApi.removeKeyDevice(id)
            // Only the contract's own answer counts as "removed": this very key device, now revoked.
            if (removed.id != id || removed.state != KeyDeviceStatus.REVOKED) return StepUpOutcome.Failed("UNEXPECTED_ANSWER")
            ServerAnswer(200)
        } catch (e: CancellationException) {
            throw e
        } catch (e: ApiException) {
            ServerAnswer.of(e)
        } catch (_: IOException) {
            return StepUpOutcome.Failed("NETWORK")
        } catch (_: Exception) {
            return StepUpOutcome.Failed("NETWORK")
        }
        // Removed, or already gone on the server (revoked between the step-up and the DELETE): the
        // person's wish is done. Only the listed answers count, and only for this very record.
        // Forget first, so the sign-out below is a plain one and unregisters push. Push goes after the
        // removal on purpose (plan B6, C19 amendment 3): a removal that fails keeps the key and its
        // subscription, and the removal notice still reaches this phone; the server deletes the
        // subscription with the revocation, so the client's DELETE afterwards is best effort.
        if (!answers.settle(rec, asked, Ask.KEY_DEVICE_REMOVE, answer).forgotten) {
            return StepUpOutcome.Failed(answer.code ?: if (answer.status == 200) "NOT_FORGOTTEN" else "HTTP_${answer.status}")
        }
        authRepository.logout()
        return out
    }

    /**
     * "Use another account" on a phone that is not signed in: the removal runs with a session of
     * its own that the app never holds ([PhoneRemoval], which also drops what the phone keeps of
     * its push registration once it is no key any more).
     */
    suspend fun removeSignedOutPhone(activity: FragmentActivity): RemovalResult = removal.run(activity)

    /**
     * This phone's key-device id when the sign-in did not record it (`/auth/me` without
     * `session.key_device_id`): the server's list marks the current one; it is kept for next time.
     * Returns the record with the id, or null.
     */
    private suspend fun currentKeyDeviceId(rec: StoredKeyDevice): StoredKeyDevice? = try {
        approvalApi.keyDevices().firstOrNull { it.isCurrent }?.id?.let { id ->
            (keyDevices.update(rec) { it.copy(device = it.device.copy(keyDeviceId = id)) } as? Change.Done)?.record
        }
    } catch (e: CancellationException) {
        throw e
    } catch (_: Exception) {
        null
    }
}
