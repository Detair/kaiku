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
 * `prefix ‖ decision ‖ lp(response)`. The caller names the key it holds ([KeyId]): the signature
 * is made with exactly that key or not at all (another key in the alias → `Failed("KEY_CHANGED")`,
 * and nothing is reported or forgotten).
 */
class DeviceSigner(
    private val keyStore: ApprovalKeyStore,
    private val verifier: UserVerifier,
    private val checks: ChallengeVerifier,
    /** Runs whenever the key named to [sign] turns out to be invalidated or missing (report + forget that key). */
    private val onKeyInvalidated: suspend (keyId: String?) -> Unit = {},
) {
    /** Runs only the checks, for screens that show the context before asking to confirm. */
    fun check(ch: ApprovalChallenge, expected: SigKind, userId: String, deviceId: String, ceremonyOrigin: String? = null): VerifiedChallenge =
        checks.verify(ch, expected, userId, deviceId, ceremonyOrigin)

    suspend fun sign(
        activity: FragmentActivity,
        ch: ApprovalChallenge,
        expected: SigKind,
        userId: String,
        deviceId: String,
        decision: Decision,
        response: ByteArray,
        prompt: UvPrompt,
        /** The key the caller holds; null: it holds none (its record names no key). */
        keyId: String?,
        /** See [ChallengeVerifier.verify]. */
        ceremonyOrigin: String? = null,
    ): SignOutcome {
        val verified = try {
            checks.verify(ch, expected, userId, deviceId, ceremonyOrigin)
        } catch (e: ChallengeRejected) {
            return SignOutcome.Rejected(e.reason)
        }
        val message = KaikuSig.message(verified.prefix, decision, response)
        val r = try {
            when {
                !keyStore.hasKey() -> UvResult.KeyInvalidated
                keyId == null || keyStore.keyId() != keyId -> KEY_CHANGED
                else -> verifier.sign(activity, prompt, message, keyId)
            }
        } catch (e: CancellationException) {
            throw e
        } catch (_: KeyInvalidatedException) {
            UvResult.KeyInvalidated
        } catch (e: Exception) {
            // An invalidated key reported in another shape still counts as invalidated; any other
            // Keystore failure (ProviderException, …) fails the signature instead of crashing.
            if (isInvalidatedKeyFailure(e)) UvResult.KeyInvalidated else UvResult.Failed("KEYSTORE_" + e.javaClass.simpleName)
        }
        // Only the key that was asked for is reported and forgotten, and only if it is still the
        // phone's key then (the handler compares): a key made meanwhile is never touched.
        if (r == UvResult.KeyInvalidated) onKeyInvalidated(keyId)
        return when (r) {
            is UvResult.Signed -> SignOutcome.Signed(Base64.getUrlEncoder().withoutPadding().encodeToString(r.der), verified)
            else -> SignOutcome.Uv(r)
        }
    }
}
