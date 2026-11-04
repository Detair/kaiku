package io.wolftown.kaiku.domain.model

import kotlinx.serialization.Serializable

/**
 * C26–C29 `StepUpGrant`: single use, bound to the session, ≤ 300 s. The grant
 * itself is held by the server; the client only learns that it exists.
 */
@Serializable
data class StepUpGrant(
    val action: String,
    val target: String,
    /** `passkey` or `device`. */
    val via: String,
    val expiresAt: String,
)

/** `POST /api/approvals/{id}/answer` → `{state, grant?, enrollment?}`. */
@Serializable
data class AnswerResult(
    val state: String,
    val grant: StepUpGrant? = null,
    val enrollment: Enrollment? = null,
)
