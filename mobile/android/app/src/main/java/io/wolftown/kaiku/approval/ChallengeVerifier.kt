package io.wolftown.kaiku.approval

import io.wolftown.kaiku.domain.model.ApprovalChallenge
import java.util.Base64

enum class RejectReason { FORMAT, MAGIC, UNKNOWN_KIND, KIND_MISMATCH, CHALLENGE_ID, ORIGIN, USER, DEVICE, EXPIRED, CONTEXT_HASH }

/** The device refuses a challenge; nothing was prompted or signed. */
class ChallengeRejected(val reason: RejectReason) : Exception("challenge rejected: $reason")

/** A challenge that passed every C26 check; screens show only [context]. */
class VerifiedChallenge(
    val challengeId: String,
    val kind: SigKind,
    val prefix: ByteArray,
    val context: SigContext,
    /** Unix seconds from the signed prefix. */
    val expiresAt: Long,
)

/**
 * C26 "Before asking for UV": magic; kind byte known; kind = JSON kind = the
 * screen ([expected]); challenge id; origin = the pinned origin; user; device;
 * not expired (60 s clock skew); `context_hash` recomputed from the JSON
 * context. Any mismatch throws [ChallengeRejected] and the caller never prompts.
 */
class ChallengeVerifier(
    private val pinnedOrigin: () -> String?,
    private val nowSeconds: () -> Long,
) {
    fun verify(ch: ApprovalChallenge, expected: SigKind, userId: String, deviceId: String): VerifiedChallenge {
        val raw = try {
            Base64.getUrlDecoder().decode(ch.signingPrefix)
        } catch (_: IllegalArgumentException) {
            throw ChallengeRejected(RejectReason.FORMAT)
        }
        if (!KaikuSig.hasMagic(raw)) {
            throw ChallengeRejected(if (raw.size < KaikuSig.MAGIC.length) RejectReason.FORMAT else RejectReason.MAGIC)
        }
        val prefix = try {
            KaikuSig.parsePrefix(raw)
        } catch (_: SigFormatException) {
            throw ChallengeRejected(RejectReason.FORMAT)
        }
        val kind = SigKind.fromByte(prefix.kind) ?: throw ChallengeRejected(RejectReason.UNKNOWN_KIND)
        if (kind != expected || ch.kind != expected.wire) throw ChallengeRejected(RejectReason.KIND_MISMATCH)
        if (prefix.challengeId != ch.challengeId) throw ChallengeRejected(RejectReason.CHALLENGE_ID)
        val pinned = pinnedOrigin() ?: throw ChallengeRejected(RejectReason.ORIGIN)
        if (prefix.origin != pinned) throw ChallengeRejected(RejectReason.ORIGIN)
        if (prefix.userId != userId) throw ChallengeRejected(RejectReason.USER)
        if (prefix.deviceId != deviceId) throw ChallengeRejected(RejectReason.DEVICE)
        if (prefix.expiresAt <= nowSeconds() - CLOCK_SKEW_SECONDS) throw ChallengeRejected(RejectReason.EXPIRED)
        val context = try {
            SigContext.fromJson(expected, ch.context)
        } catch (_: SigFormatException) {
            throw ChallengeRejected(RejectReason.FORMAT)
        }
        if (!KaikuSig.contextHash(context).contentEquals(prefix.contextHash)) {
            throw ChallengeRejected(RejectReason.CONTEXT_HASH)
        }
        return VerifiedChallenge(ch.challengeId, kind, raw, context, prefix.expiresAt)
    }

    companion object {
        /** Device clock skew allowed for `expires_at` (C26). */
        const val CLOCK_SKEW_SECONDS = 60L
    }
}
