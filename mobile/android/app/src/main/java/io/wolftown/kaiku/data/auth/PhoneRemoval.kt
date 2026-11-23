package io.wolftown.kaiku.data.auth

import androidx.fragment.app.FragmentActivity
import io.wolftown.kaiku.R
import io.wolftown.kaiku.approval.ChallengeRejected
import io.wolftown.kaiku.approval.ClientClass
import io.wolftown.kaiku.approval.Decision
import io.wolftown.kaiku.approval.DeviceIdProvider
import io.wolftown.kaiku.approval.DeviceLoginContext
import io.wolftown.kaiku.approval.DeviceSigner
import io.wolftown.kaiku.approval.DeviceStepUpContext
import io.wolftown.kaiku.approval.SigKind
import io.wolftown.kaiku.approval.SignOutcome
import io.wolftown.kaiku.approval.StepUpText
import io.wolftown.kaiku.approval.UvPrompt
import io.wolftown.kaiku.approval.UvResult
import io.wolftown.kaiku.data.api.ApiException
import io.wolftown.kaiku.data.api.ApprovalApi
import io.wolftown.kaiku.data.api.AuthApi
import io.wolftown.kaiku.data.api.RemovalApi
import io.wolftown.kaiku.data.keydevice.Ask
import io.wolftown.kaiku.data.keydevice.Change
import io.wolftown.kaiku.data.keydevice.KeyAnswers
import io.wolftown.kaiku.data.keydevice.KeyDevicePhase
import io.wolftown.kaiku.data.keydevice.KeyDeviceRepository
import io.wolftown.kaiku.data.keydevice.ServerAnswer
import io.wolftown.kaiku.data.keydevice.Verdict
import io.wolftown.kaiku.di.AuthCoroutineScope
import io.wolftown.kaiku.domain.KaikuPayloads
import io.wolftown.kaiku.domain.model.AuthResponse
import io.wolftown.kaiku.domain.model.KeyDeviceStatus
import io.wolftown.kaiku.domain.model.StepUpAction
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeoutOrNull
import java.io.IOException
import java.util.logging.Logger
import javax.inject.Inject
import kotlin.coroutines.cancellation.CancellationException

/**
 * What a phone that was removed as a key while signed out does about its push registration: only
 * what the phone itself still keeps of it, for the account it was a key of. It gets the account's
 * id and nothing else of the removal.
 */
fun interface RemovedPhonePush {
    suspend fun drop(userId: String)
}

/** How a removal of this phone ended. */
sealed interface RemovalResult {
    /** The server removed this phone as a key; its key and record are forgotten. */
    data object Removed : RemovalResult
    data object Cancelled : RemovalResult
    /** This phone turned out to be no key any more ([invalidated]: Android invalidated the key). */
    data class KeyGone(val invalidated: Boolean) : RemovalResult
    /** Nothing was removed; [code] says why (a server code, `NETWORK`, `UV_LOCKED_OUT`, …). */
    data class Failed(val code: String) : RemovalResult
}

/**
 * "Use another account" on a signed-out key phone: the phone stops being the key first, and
 * removing it needs a session. That session is this operation's own. Its tokens live in one local
 * value of [run] and nowhere else: they are not written to storage, not made the app's sign-in,
 * and no other part of the app (the connection, push, token renewal, any screen) ever sees them.
 * With them, through [RemovalApi] (every call names its server and bearer), the operation reads
 * the account, confirms the removal with the key and deletes the key device. However it ends
 * (removed, refused, cancelled, its screen gone), nothing of it is on disk; unless the server ended
 * the session with the removal, it is revoked best effort on the application scope. If the app
 * dies half way, that session stays valid on the server until it runs out, with no holder.
 *
 * Nothing shared is touched, so a sign-in that happens meanwhile is not affected, and there is
 * nothing to clean up at the next start. Every request goes to the server of the record captured
 * at the start, and only that record can be forgotten (through [KeyAnswers.settle]).
 */
class PhoneRemoval @Inject constructor(
    private val approvalApi: ApprovalApi,
    private val api: RemovalApi,
    private val authApi: AuthApi,
    private val signer: DeviceSigner,
    private val keyDevices: KeyDeviceRepository,
    private val answers: KeyAnswers,
    private val deviceIds: DeviceIdProvider,
    private val removedPush: RemovedPhonePush,
    @AuthCoroutineScope private val appScope: CoroutineScope,
) {
    private companion object {
        val logger: Logger = Logger.getLogger("PhoneRemoval")
        const val REVOKE_TIMEOUT_MS = 10_000L
    }

    suspend fun run(activity: FragmentActivity): RemovalResult {
        val rec = keyDevices.record.value?.takeIf { it.phase == KeyDevicePhase.ACTIVE } ?: return RemovalResult.KeyGone(invalidated = false)
        val origin = KaikuPayloads.canonicalOrigin(rec.device.origin)?.takeIf { it.startsWith("https://") }
            ?: return RemovalResult.Failed("OTHER_SERVER")
        val userId = rec.device.userId
        // The install id the key device was enrolled under, whatever id the app has now; without
        // it nothing is asked (an id the app has now may have been made later).
        val deviceId = rec.installId ?: return RemovalResult.Failed("INSTALL_ID_LOST")
        // The removal's credentials: here, and in the calls below that are handed them, only.
        var session: AuthResponse? = null
        var endedByServer = false
        var ask = Ask.DEVICE_LOGIN_CHALLENGE
        // The record every answer is settled on: [rec], or the same record no longer provisional.
        var current = rec
        try {
            val ch = approvalApi.deviceLoginChallenge(userId, deviceId, origin)
            val login = try {
                signer.check(ch, SigKind.DEVICE_LOGIN, userId, deviceId)
            } catch (e: ChallengeRejected) {
                return RemovalResult.Failed("CHALLENGE_${e.reason}")
            }
            if ((login.context as? DeviceLoginContext)?.clientClass != ClientClass.ANDROID) {
                return RemovalResult.Failed("CHALLENGE_CONTEXT_MISMATCH")
            }
            val first = signer.sign(
                activity, ch, SigKind.DEVICE_LOGIN, userId, deviceId, Decision.APPROVE, ByteArray(0),
                UvPrompt(
                    R.string.security_uv_title,
                    activity.getString(R.string.security_uv_for_removal),
                    activity.getString(R.string.security_uv_for_removal_detail),
                ),
                rec.keyId,
            )
            val loginSignature = when (first) {
                is SignOutcome.Signed -> first.signatureB64u
                is SignOutcome.Rejected -> return RemovalResult.Failed("CHALLENGE_${first.reason}")
                is SignOutcome.Uv -> return onUv(first.result)
            }
            ask = Ask.DEVICE_LOGIN
            val s = approvalApi.deviceLogin(ch.challengeId, loginSignature, origin)
            session = s
            // The server accepted a sign-in with this very key for this install: the key is no
            // longer a key device only by the server's word about this install. If the phone cannot
            // note that, it could not take the removal's answer either: nothing is removed, and the
            // session ends like after any other early stop.
            if (rec.provisionalFrom != null) {
                val noted = keyDevices.update(rec) { it.copy(provisionalFrom = null) }
                current = (noted as? Change.Done)?.record ?: return RemovalResult.Failed("NOT_SAVED")
            }
            ask = Ask.STEP_UP
            val me = api.me(origin, s.accessToken)
            if (me.id != userId) return RemovalResult.Failed("OTHER_ACCOUNT")
            val own = me.session
            if (own == null || own.assurance != "full") return RemovalResult.Failed("FULL_SESSION_REQUIRED")
            val id = own.keyDeviceId
                ?: api.keyDevices(origin, s.accessToken).firstOrNull { it.isCurrent }?.id
                ?: return RemovalResult.Failed("NO_KEY_DEVICE_ID")
            val action = StepUpAction.KEY_DEVICE_REMOVE
            val ch2 = api.stepUp(origin, s.accessToken, action.wire, id)
            val stepUp = try {
                signer.check(ch2, SigKind.DEVICE_STEPUP, userId, deviceId)
            } catch (e: ChallengeRejected) {
                return RemovalResult.Failed("CHALLENGE_${e.reason}")
            }
            val ctx = stepUp.context as? DeviceStepUpContext
            if (ctx == null || ctx.sid != own.id || ctx.action != action.wire || ctx.target != id) {
                return RemovalResult.Failed("CONTEXT_MISMATCH")
            }
            val (res, takesLabel) = StepUpText.prompt(action)
            // Only the signed label is shown (C26: display only the parsed context).
            val subtitle = if (takesLabel) activity.getString(res, ctx.targetLabel) else activity.getString(res)
            val second = signer.sign(
                activity, ch2, SigKind.DEVICE_STEPUP, userId, deviceId, Decision.APPROVE, ByteArray(0),
                UvPrompt(R.string.security_uv_title, subtitle), rec.keyId,
            )
            val stepUpSignature = when (second) {
                is SignOutcome.Signed -> second.signatureB64u
                is SignOutcome.Rejected -> return RemovalResult.Failed("CHALLENGE_${second.reason}")
                is SignOutcome.Uv -> return onUv(second.result)
            }
            api.answer(origin, s.accessToken, ch2.challengeId, Decision.APPROVE.wire, "", stepUpSignature).grant
                ?: return RemovalResult.Failed("NO_GRANT")
            ask = Ask.KEY_DEVICE_REMOVE
            val removed = api.removeKeyDevice(origin, s.accessToken, id)
            // Only the contract's own answer counts as "removed": this very key device, now revoked.
            // A bare success without it decides nothing (and the session is ended like after any failure).
            if (removed.id != id || removed.state != KeyDeviceStatus.REVOKED) return RemovalResult.Failed("UNEXPECTED_ANSWER")
            // The server ended this session with the removal (the revocation's cascade).
            endedByServer = true
            if (!answers.settle(current, origin, ask, ServerAnswer(200)).forgotten) {
                // Removed on the server. Still this record, so the phone could not clear it: said so
                // (the next tap is answered as for a removed key, and the phone forgets it then). A
                // record that changed meanwhile is another one, and nothing of it is forgotten.
                val same = keyDevices.record.value?.sameAs(current) == true
                return RemovalResult.Failed(if (same) "REMOVED_NOT_SAVED" else "NOT_FORGOTTEN")
            }
            // The server dropped the phone's push subscription with the removal; what the phone
            // still keeps of that registration goes too (no request, nothing of another session).
            try {
                removedPush.drop(userId)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                logger.warning("Push registration not dropped after the removal: ${e.javaClass.simpleName}")
            }
            return RemovalResult.Removed
        } catch (e: CancellationException) {
            throw e
        } catch (e: ApiException) {
            // The server's own answers for a key device that is gone, asked on the key's server
            // about the captured record: the phone forgets that record (and only that one).
            val settled = answers.settle(current, origin, ask, ServerAnswer.of(e))
            if (settled.verdict == Verdict.KEY_GONE) {
                return if (settled.forgotten) RemovalResult.KeyGone(invalidated = false) else RemovalResult.Failed("NOT_FORGOTTEN")
            }
            return RemovalResult.Failed(e.normalizedCode ?: "HTTP_${e.status.value}")
        } catch (_: IOException) {
            return RemovalResult.Failed("NETWORK")
        } catch (e: Exception) {
            // An unreadable answer (a captive portal's page) or a transport failure: no crash.
            logger.warning("Removal failed: ${e.javaClass.simpleName}")
            return RemovalResult.Failed("NETWORK")
        } finally {
            val left = session
            if (left != null && !endedByServer) revoke(origin, left)
        }
    }

    /** Ends the removal's own session on its server; never waited for, never cancelled with the screen. */
    private fun revoke(origin: String, s: AuthResponse) {
        appScope.launch {
            try {
                withTimeoutOrNull(REVOKE_TIMEOUT_MS) { authApi.logoutSession(origin, s.accessToken, s.refreshToken) }
            } catch (e: Exception) {
                logger.warning("Removal session not ended (best effort): ${e.javaClass.simpleName}")
            }
        }
    }

    private fun onUv(r: UvResult): RemovalResult = when (r) {
        UvResult.Cancelled -> RemovalResult.Cancelled
        UvResult.LockedOut -> RemovalResult.Failed("UV_LOCKED_OUT")
        // The signer already forgot the key it was asked to sign with, if it was still the phone's.
        UvResult.KeyInvalidated -> RemovalResult.KeyGone(invalidated = true)
        is UvResult.Failed -> RemovalResult.Failed(r.code)
        is UvResult.Signed -> RemovalResult.Failed("UNEXPECTED")
    }
}
