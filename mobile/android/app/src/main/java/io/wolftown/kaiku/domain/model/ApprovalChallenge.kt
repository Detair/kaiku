package io.wolftown.kaiku.domain.model

import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonObject

/**
 * C26 `ApprovalChallenge` (server → device). [signingPrefix] is b64u of M up to
 * and including `context_hash`; [context] is the JSON form of the canonical
 * context C. Nothing here is trusted before
 * [io.wolftown.kaiku.approval.ChallengeVerifier] checked it; screens show only
 * the verified, parsed context.
 */
@Serializable
data class ApprovalChallenge(
    val challengeId: String,
    val kind: String,
    val variant: String? = null,
    val expiresAt: String,
    val signingPrefix: String,
    val context: JsonObject,
)
