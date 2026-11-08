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

/**
 * `POST /api/pc-setups/{id}/answer` → `{attempt_id, code?, code_expires_at?}`. An approval carries the
 * six digits the person types on the PC; [toString] hides them.
 */
@Serializable
data class PcSetupAnswer(
    val attemptId: String,
    val code: String? = null,
    val codeExpiresAt: String? = null,
) {
    override fun toString() = "PcSetupAnswer(attemptId=$attemptId)"
}

/** `GET /api/pc-setups/{attempt_id}` → the SM5 `state` and why it ended. */
@Serializable
data class PcSetupPhoneStatus(val state: String, val reason: String? = null)
