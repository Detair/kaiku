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
import io.wolftown.kaiku.data.local.AuthSession
import io.wolftown.kaiku.data.keydevice.Ask
import io.wolftown.kaiku.data.keydevice.Change
import io.wolftown.kaiku.data.keydevice.KeyAnswers
import io.wolftown.kaiku.data.keydevice.KeyDevicePhase
import io.wolftown.kaiku.data.keydevice.KeyDeviceRepository
import io.wolftown.kaiku.data.keydevice.ServerAnswer
import io.wolftown.kaiku.data.keydevice.StoredKeyDevice
import io.wolftown.kaiku.data.keydevice.Verdict
import io.wolftown.kaiku.domain.KaikuPayloads
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
    /** The server's challenge ran out while the prompt was open (`APPROVAL_EXPIRED`); trying again starts a new one. */
    data object Expired : DeviceSignInResult
    data object Network : DeviceSignInResult
    data class Failed(val code: String) : DeviceSignInResult
}

/** Why the phone signs in with its key; the screen-lock prompt says it. */
enum class SignInPurpose {
    /** "Confirm it's you" after a lapse. */
    SIGN_IN,
    /** The last step of a setup: its own fresh challenge, so a second prompt (C26 `device_login`). */
    AFTER_SETUP,
    /** The renewal before the 90-day cap. */
    STAY_SIGNED_IN,
}

/** The prompt's subtitle and optional description for [purpose]. */
internal fun signInPromptText(purpose: SignInPurpose): Pair<Int, Int?> = when (purpose) {
    SignInPurpose.SIGN_IN -> R.string.security_uv_sign_in to null
    SignInPurpose.AFTER_SETUP -> R.string.security_uv_after_setup to R.string.security_uv_after_setup_detail
    SignInPurpose.STAY_SIGNED_IN -> R.string.security_uv_stay_signed_in to null
}

/**
 * L2-A11: device-key sign-in (C26 `device_login`) → a full session. Used after a
 * lapse ("Confirm it's you"), at the end of a setup, and for the renewal before
 * the 90-day cap. Never a password.
 *
 * The whole sign-in is about the record captured at its start: both requests go to that record's
 * server, an answer that says the key is gone forgets only that record ([KeyAnswers.settle]), the
 * session is installed only while that record is still the stored one, and success is reported
 * only after the record took the sign-in. A key forgotten or replaced meanwhile never ends as
 * "signed in".
 */
class DeviceSignIn @Inject constructor(
    private val approvalApi: ApprovalApi,
    private val signer: DeviceSigner,
    private val keyDevices: KeyDeviceRepository,
    private val deviceIds: DeviceIdProvider,
    private val authRepository: AuthRepository,
    private val answers: KeyAnswers,
) {
    private companion object {
        val logger: Logger = Logger.getLogger("DeviceSignIn")
    }

    suspend fun signIn(activity: FragmentActivity, purpose: SignInPurpose = SignInPurpose.SIGN_IN): DeviceSignInResult {
        val rec = keyDevices.record.value?.takeIf { it.phase == KeyDevicePhase.ACTIVE }
            ?: return DeviceSignInResult.Failed("NO_KEY_DEVICE")
        val local = rec.device
        val origin = KaikuPayloads.canonicalOrigin(local.origin)?.takeIf { it.startsWith("https://") }
            ?: return DeviceSignInResult.Failed("OTHER_SERVER")
        // Captured before anything is sent: a logout, a server change or another session meanwhile
        // means the answer is not installed (and is ended on the server).
        val start = authRepository.beginDeviceSignIn(local.origin) ?: return DeviceSignInResult.Failed("OTHER_SERVER")
        // The install id the key device was enrolled under, whatever id the app has now.
        val deviceId = rec.installId ?: deviceIds.id()
        var ask = Ask.DEVICE_LOGIN_CHALLENGE
        return try {
            val ch = approvalApi.deviceLoginChallenge(local.userId, deviceId, origin)
            // The context is checked before the prompt: this app signs in only as an Android client.
            val verified = try {
                signer.check(ch, SigKind.DEVICE_LOGIN, local.userId, deviceId)
            } catch (e: ChallengeRejected) {
                return DeviceSignInResult.Failed("CHALLENGE_${e.reason}")
            }
            if ((verified.context as? DeviceLoginContext)?.clientClass != ClientClass.ANDROID) {
                return DeviceSignInResult.Failed("CHALLENGE_CONTEXT_MISMATCH")
            }
            val (subtitle, detail) = signInPromptText(purpose)
            val out = signer.sign(
                activity, ch, SigKind.DEVICE_LOGIN, local.userId, deviceId, Decision.APPROVE, ByteArray(0),
                UvPrompt(R.string.security_uv_title, activity.getString(subtitle), detail?.let(activity::getString)),
                rec.keyId,
            )
            when (out) {
                is SignOutcome.Rejected -> DeviceSignInResult.Failed("CHALLENGE_${out.reason}")
                is SignOutcome.Uv -> onUv(out.result)
                is SignOutcome.Signed -> {
                    ask = Ask.DEVICE_LOGIN
                    val auth = approvalApi.deviceLogin(ch.challengeId, out.signatureB64u, origin)
                    var installed: AuthSession.LoggedIn? = null
                    val user = authRepository.installDeviceSession(
                        auth, local.userId, start,
                        stillThisKey = { keyDevices.record.value sameAsRecord rec },
                        installedAs = { installed = it },
                    )
                        .getOrElse {
                            logger.warning("Device session not installed: ${it.javaClass.simpleName}")
                            return if (keyDevices.record.value sameAsRecord rec) {
                                DeviceSignInResult.Failed("SESSION_NOT_INSTALLED")
                            } else {
                                DeviceSignInResult.Failed("NO_KEY_DEVICE")
                            }
                        }
                    val cap = auth.absoluteExpiresAt?.let { runCatching { Instant.parse(it) }.getOrNull() }
                    val taken = keyDevices.update(rec) {
                        it.copy(
                            // A signature of this very key was accepted: the record is a key device for good.
                            provisionalFrom = null,
                            device = it.device.copy(keyDeviceId = user.session?.keyDeviceId ?: it.device.keyDeviceId, claimSecret = null),
                            readyAt = null,
                            sessionCap = cap,
                            // The server just took this id for the key device: it is the record's from now on.
                            installId = it.installId ?: deviceId,
                            // The server answered: nothing is noted against it any more.
                            unanswered = null,
                        )
                    }
                    when (taken) {
                        is Change.Done -> Unit
                        Change.Mismatch -> {
                            // The key was forgotten or replaced while the session was being installed:
                            // that session is not a sign-in of this phone's key any more. Only that
                            // session ends, never one that came after it.
                            installed?.let { authRepository.logoutIfCurrent(it) }
                            return DeviceSignInResult.Failed("NO_KEY_DEVICE")
                        }
                        // The record is still this one; only what is kept beside it (the key-device
                        // id, the session's cap) did not reach storage. The sign-in stands.
                        Change.WriteFailed, Change.Unreadable -> logger.warning("Sign-in not noted on the key record: ${taken.javaClass.simpleName}")
                    }
                    DeviceSignInResult.Success
                }
            }
        } catch (e: CancellationException) {
            throw e
        } catch (e: ApiException) {
            // Only the server's own two answers for a removed key forget it (any other status
            // keeps it), asked on the key's server about the captured record.
            val settled = answers.settle(rec, origin, ask, ServerAnswer.of(e))
            if (settled.forgotten) return DeviceSignInResult.Revoked
            // The server's word that this install is a key device was about another key: the
            // record goes back to the unfinished setup it was, and never takes that word again.
            val from = rec.provisionalFrom
            if (from != null && (settled.verdict == Verdict.KEY_GONE || e.normalizedCode == "SIGNATURE_INVALID")) {
                keyDevices.replace(rec, rec.copy(phase = from, provisionalFrom = null, notThisKey = true))
                return DeviceSignInResult.Failed("NOT_THIS_KEY")
            }
            // "No key device for this install" for a key whose install id is not known: kept.
            if (settled.verdict == Verdict.KEY_GONE && rec.installIdLost) return DeviceSignInResult.Failed("INSTALL_ID_LOST")
            if (settled.verdict == Verdict.KEY_GONE) return DeviceSignInResult.Failed("NO_KEY_DEVICE")
            when (e.normalizedCode) {
                "KEY_DEVICE_PENDING" -> {
                    val readyAt = e.fields.readyAt?.let { runCatching { Instant.parse(it) }.getOrNull() }
                    if (readyAt == null) {
                        DeviceSignInResult.Failed("KEY_DEVICE_PENDING")
                    } else {
                        keyDevices.update(rec) { it.copy(phase = KeyDevicePhase.PENDING, readyAt = readyAt) }
                        DeviceSignInResult.Pending(readyAt)
                    }
                }
                "ACCOUNT_BANNED" -> DeviceSignInResult.Banned
                // A prompt left open past the challenge's 120 s: nothing changed, a retry asks anew.
                "APPROVAL_EXPIRED" -> DeviceSignInResult.Expired
                else -> DeviceSignInResult.Failed(e.normalizedCode ?: "HTTP_${e.status.value}")
            }
        } catch (_: IOException) {
            DeviceSignInResult.Network
        } catch (e: Exception) {
            // An unreadable answer (a captive portal's page) or a non-IO transport failure: no crash.
            logger.warning("Device sign-in failed: ${e.javaClass.simpleName}")
            DeviceSignInResult.Network
        }
    }

    private infix fun StoredKeyDevice?.sameAsRecord(rec: StoredKeyDevice): Boolean = this != null && this sameAs rec

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
