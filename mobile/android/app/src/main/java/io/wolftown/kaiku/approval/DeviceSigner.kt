package io.wolftown.kaiku.approval

import androidx.fragment.app.FragmentActivity
import io.wolftown.kaiku.domain.model.ApprovalChallenge
import java.util.Base64
import kotlin.coroutines.cancellation.CancellationException

sealed interface SignOutcome {
    /** [signatureB64u]: b64u ASN.1 DER ECDSA over M; [verified] is what the screen showed and signed. */
    class Signed(val signatureB64u: String, val verified: VerifiedChallenge) : SignOutcome {
        override fun toString(): String = "Signed(${verified.kind})"
    }
    data class Rejected(val reason: RejectReason) : SignOutcome
    data class Uv(val result: UvResult) : SignOutcome
}

/**
 * Every device signature goes through here: the C26 checks first (a rejected
 * challenge is never prompted), then the UV prompt and the signature over
 * `prefix ‖ decision ‖ lp(response)`.
 */
class DeviceSigner(
    private val keyStore: ApprovalKeyStore,
    private val verifier: UserVerifier,
    private val checks: ChallengeVerifier,
    /** Runs once whenever the key turns out to be invalidated or missing (report + forget). */
    private val onKeyInvalidated: suspend () -> Unit = {},
) {
    /** Runs only the checks, for screens that show the context before asking to confirm. */
    fun check(ch: ApprovalChallenge, expected: SigKind, userId: String, deviceId: String): VerifiedChallenge =
        checks.verify(ch, expected, userId, deviceId)

    suspend fun sign(
        activity: FragmentActivity,
        ch: ApprovalChallenge,
        expected: SigKind,
        userId: String,
        deviceId: String,
        decision: Decision,
        response: ByteArray,
        prompt: UvPrompt,
    ): SignOutcome {
        val verified = try {
            checks.verify(ch, expected, userId, deviceId)
        } catch (e: ChallengeRejected) {
            return SignOutcome.Rejected(e.reason)
        }
        val message = KaikuSig.message(verified.prefix, decision, response)
        val r = try {
            if (!keyStore.hasKey()) UvResult.KeyInvalidated else verifier.sign(activity, prompt, message)
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            // Any other Keystore failure (ProviderException, UnrecoverableKeyException, …) fails the
            // signature instead of crashing the app.
            UvResult.Failed("KEYSTORE_" + e.javaClass.simpleName)
        }
        if (r == UvResult.KeyInvalidated) onKeyInvalidated()
        return when (r) {
            is UvResult.Signed -> SignOutcome.Signed(Base64.getUrlEncoder().withoutPadding().encodeToString(r.der), verified)
            else -> SignOutcome.Uv(r)
        }
    }
}
