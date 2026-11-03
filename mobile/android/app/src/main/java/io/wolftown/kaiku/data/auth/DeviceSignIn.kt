package io.wolftown.kaiku.data.auth

import androidx.fragment.app.FragmentActivity
import io.wolftown.kaiku.R
import io.wolftown.kaiku.approval.Decision
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
        val deviceId = deviceIds.id()
        return try {
            val ch = approvalApi.deviceLoginChallenge(local.userId, deviceId)
            val out = signer.sign(
                activity, ch, SigKind.DEVICE_LOGIN, local.userId, deviceId, Decision.APPROVE, ByteArray(0),
                UvPrompt(R.string.security_uv_title, activity.getString(R.string.security_uv_sign_in)),
            )
            when (out) {
                is SignOutcome.Rejected -> DeviceSignInResult.Failed("CHALLENGE_${out.reason}")
                is SignOutcome.Uv -> onUv(out.result, local.keyDeviceId)
                is SignOutcome.Signed -> {
                    val auth = approvalApi.deviceLogin(ch.challengeId, out.signatureB64u)
                    val user = authRepository.installDeviceSession(auth, local.userId).getOrElse {
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

    private suspend fun onUv(r: UvResult, keyDeviceId: String?): DeviceSignInResult = when (r) {
        UvResult.Cancelled -> DeviceSignInResult.Cancelled
        UvResult.LockedOut -> DeviceSignInResult.Failed("UV_LOCKED_OUT")
        is UvResult.Failed -> DeviceSignInResult.Failed(r.code)
        is UvResult.Signed -> DeviceSignInResult.Failed("UNEXPECTED")
        UvResult.KeyInvalidated -> {
            // Unsigned report (it only removes); without a live session the app just shows setup again.
            if (keyDeviceId != null && authRepository.hasLiveSession()) {
                try {
                    approvalApi.reportInvalidated(keyDeviceId)
                } catch (e: CancellationException) {
                    throw e
                } catch (e: Exception) {
                    logger.warning("Invalidated key not reported: ${e.javaClass.simpleName}")
                }
            }
            keyDevices.forget()
            DeviceSignInResult.KeyInvalidated
        }
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
