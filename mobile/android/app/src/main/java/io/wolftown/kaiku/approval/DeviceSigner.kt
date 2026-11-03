package io.wolftown.kaiku.approval

import androidx.fragment.app.FragmentActivity
import io.wolftown.kaiku.domain.model.ApprovalChallenge
import java.util.Base64

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
        if (!keyStore.hasKey()) return SignOutcome.Uv(UvResult.KeyInvalidated)
        val message = KaikuSig.message(verified.prefix, decision, response)
        return when (val r = verifier.sign(activity, prompt, message)) {
            is UvResult.Signed -> SignOutcome.Signed(Base64.getUrlEncoder().withoutPadding().encodeToString(r.der), verified)
            else -> SignOutcome.Uv(r)
        }
    }
}
