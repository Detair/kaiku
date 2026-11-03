package io.wolftown.kaiku.data.auth

import androidx.fragment.app.FragmentActivity
import io.wolftown.kaiku.R
import io.wolftown.kaiku.approval.ChallengeRejected
import io.wolftown.kaiku.approval.ClientClass
import io.wolftown.kaiku.approval.Decision
import io.wolftown.kaiku.approval.DeviceLoginContext
import io.wolftown.kaiku.approval.DeviceIdProvider
import io.wolftown.kaiku.approval.DeviceSigner
import io.wolftown.kaiku.approval.SigKind
import io.wolftown.kaiku.approval.SignOutcome
import io.wolftown.kaiku.approval.UvPrompt
import io.wolftown.kaiku.approval.UvResult
import io.wolftown.kaiku.data.api.ApiException
import io.wolftown.kaiku.data.api.ApprovalApi
import io.wolftown.kaiku.data.keydevice.KeyDeviceRepository
import io.wolftown.kaiku.data.repository.AuthRepository
import java.io.IOException
import java.time.Duration
import java.time.Instant
import java.util.logging.Logger
import javax.inject.Inject
import kotlin.coroutines.cancellation.CancellationException

sealed interface DeviceSignInResult {
    data object Success : DeviceSignInResult
    data object Cancelled : DeviceSignInResult
    data class Pending(val readyAt: Instant) : DeviceSignInResult
    data object Revoked : DeviceSignInResult
    /** Android invalidated the key (screen lock removed): reported if possible, forgotten. */
    data object KeyInvalidated : DeviceSignInResult
    data object Banned : DeviceSignInResult
    data object Network : DeviceSignInResult
    data class Failed(val code: String) : DeviceSignInResult
}

/**
 * L2-A11: device-key sign-in (C26 `device_login`) → a full session. Used after a
 * lapse ("Confirm it's you"), at the end of a setup, and for the renewal before
 * the 90-day cap. Never a password.
 */
class DeviceSignIn @Inject constructor(
    private val approvalApi: ApprovalApi,
    private val signer: DeviceSigner,
    private val keyDevices: KeyDeviceRepository,
    private val deviceIds: DeviceIdProvider,
    private val authRepository: AuthRepository,
) {
    private companion object {
        val logger: Logger = Logger.getLogger("DeviceSignIn")
    }

    suspend fun signIn(activity: FragmentActivity): DeviceSignInResult {
        val local = keyDevices.local() ?: return DeviceSignInResult.Failed("NO_KEY_DEVICE")
        // Captured before anything is sent: a logout, a server change or another session meanwhile
        // means the answer is not installed (and is ended on the server).
        val start = authRepository.beginDeviceSignIn(local.origin) ?: return DeviceSignInResult.Failed("OTHER_SERVER")
        val deviceId = deviceIds.id()
        return try {
            val ch = approvalApi.deviceLoginChallenge(local.userId, deviceId)
            // The context is checked before the prompt: this app signs in only as an Android client.
            val verified = try {
                signer.check(ch, SigKind.DEVICE_LOGIN, local.userId, deviceId)
            } catch (e: ChallengeRejected) {
                return DeviceSignInResult.Failed("CHALLENGE_${e.reason}")
            }
            if ((verified.context as? DeviceLoginContext)?.clientClass != ClientClass.ANDROID) {
                return DeviceSignInResult.Failed("CHALLENGE_CONTEXT_MISMATCH")
            }
            val out = signer.sign(
                activity, ch, SigKind.DEVICE_LOGIN, local.userId, deviceId, Decision.APPROVE, ByteArray(0),
                UvPrompt(R.string.security_uv_title, activity.getString(R.string.security_uv_sign_in)),
            )
            when (out) {
                is SignOutcome.Rejected -> DeviceSignInResult.Failed("CHALLENGE_${out.reason}")
                is SignOutcome.Uv -> onUv(out.result)
                is SignOutcome.Signed -> {
                    val auth = approvalApi.deviceLogin(ch.challengeId, out.signatureB64u)
                    val user = authRepository.installDeviceSession(auth, local.userId, start).getOrElse {
                        logger.warning("Device session not installed: ${it.javaClass.simpleName}")
                        return DeviceSignInResult.Failed("SESSION_NOT_INSTALLED")
                    }
                    keyDevices.markActive(local.userId, user.session?.keyDeviceId)
                    keyDevices.recordSessionCap(auth.absoluteExpiresAt?.let { runCatching { Instant.parse(it) }.getOrNull() })
                    DeviceSignInResult.Success
                }
            }
        } catch (e: CancellationException) {
            throw e
        } catch (e: ApiException) {
            when (e.normalizedCode) {
                "KEY_DEVICE_REVOKED", "KEY_DEVICE_UNKNOWN" -> {
                    keyDevices.forget()
                    DeviceSignInResult.Revoked
                }
                "KEY_DEVICE_PENDING" -> {
                    val readyAt = e.fields.readyAt?.let { runCatching { Instant.parse(it) }.getOrNull() }
                    if (readyAt == null) {
                        DeviceSignInResult.Failed("KEY_DEVICE_PENDING")
                    } else {
                        keyDevices.markPending(local, readyAt)
                        DeviceSignInResult.Pending(readyAt)
                    }
                }
                "ACCOUNT_BANNED" -> DeviceSignInResult.Banned
                else -> DeviceSignInResult.Failed(e.normalizedCode ?: "HTTP_${e.status.value}")
            }
        } catch (_: IOException) {
            DeviceSignInResult.Network
        }
    }

    private fun onUv(r: UvResult): DeviceSignInResult = when (r) {
        UvResult.Cancelled -> DeviceSignInResult.Cancelled
        UvResult.LockedOut -> DeviceSignInResult.Failed("UV_LOCKED_OUT")
        is UvResult.Failed -> DeviceSignInResult.Failed(r.code)
        is UvResult.Signed -> DeviceSignInResult.Failed("UNEXPECTED")
        // DeviceSigner already reported (with a live session) and forgot the device (KeyInvalidation).
        UvResult.KeyInvalidated -> DeviceSignInResult.KeyInvalidated
    }
}

/**
 * B.4 renewal (coordinator call 5): at a normal app open within the last 7 days
 * before the absolute cap, at most once per 24 h, never from a call or a
 * notification.
 */
object RenewalPolicy {
    private val WINDOW: Duration = Duration.ofDays(7)
    private val INTERVAL: Duration = Duration.ofHours(24)

    fun shouldRenew(now: Instant, absoluteExpiresAt: Instant?, lastPromptAt: Instant?, fromCallOrNotification: Boolean): Boolean {
        if (fromCallOrNotification || absoluteExpiresAt == null) return false
        if (now.isBefore(absoluteExpiresAt.minus(WINDOW))) return false
        return lastPromptAt == null || !now.isBefore(lastPromptAt.plus(INTERVAL))
    }
}
