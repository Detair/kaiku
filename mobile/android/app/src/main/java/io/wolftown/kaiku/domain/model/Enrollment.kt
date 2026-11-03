package io.wolftown.kaiku.domain.model

import kotlinx.serialization.Serializable

@Serializable(with = EnrollmentStateSerializer::class)
enum class EnrollmentState(val wire: String) {
    ISSUED("issued"), CLAIMED("claimed"), DONE("done"), CANCELLED("cancelled"), EXPIRED("expired"), FAILED("failed"),
    UNKNOWN("unknown"),
}

object EnrollmentStateSerializer : LenientEnumSerializer<EnrollmentState>(
    "EnrollmentState", EnrollmentState.values(), EnrollmentState.UNKNOWN, { it.wire },
)

@Serializable(with = KeyDeviceStatusSerializer::class)
enum class KeyDeviceStatus(val wire: String) { PENDING("pending"), ACTIVE("active"), REVOKED("revoked"), UNKNOWN("unknown") }

object KeyDeviceStatusSerializer : LenientEnumSerializer<KeyDeviceStatus>(
    "KeyDeviceStatus", KeyDeviceStatus.values(), KeyDeviceStatus.UNKNOWN, { it.wire },
)

/** `{user_id, username, display_name}`: the account an enrolment, request or notice is about. */
@Serializable
data class AccountRef(val userId: String, val username: String, val displayName: String = "")

/** C26 `EnrollmentIssued`. [setupToken] and [setupUrl] carry the single-use setup secret. */
@Serializable
data class EnrollmentIssued(
    val enrollmentId: String,
    val setupToken: String,
    val setupUrl: String,
    val expiresAt: String,
) {
    override fun toString(): String = "EnrollmentIssued(enrollmentId=$enrollmentId, expiresAt=$expiresAt)"
}

@Serializable
data class EnrollmentClaimedBy(val authenticatorType: String, val label: String, val keyHash: String = "")

/** C26 `Enrollment`; issuer screens show [claimed]'s label as "a device calling itself …" (F5a). */
@Serializable
data class Enrollment(
    val id: String,
    val variant: String = "",
    val state: EnrollmentState,
    val subject: AccountRef,
    val claimed: EnrollmentClaimedBy? = null,
    val triesLeft: Int = 0,
    val codeExpiresAt: String? = null,
    val expiresAt: String = "",
    val requestId: String? = null,
)

/** `POST /auth/enrollments/open` → `{server_name, enrollment_nonce, expires_at}`. */
@Serializable
data class EnrollmentOpen(val serverName: String, val enrollmentNonce: String, val expiresAt: String)

/** `POST /auth/enrollments/pop-challenge` → `{challenge, subject}`. */
@Serializable
data class PopChallenge(val challenge: ApprovalChallenge, val subject: AccountRef)

/** `POST /auth/enrollments/claim` body. */
@Serializable
data class EnrollmentClaim(
    val setupToken: String,
    val challengeId: String,
    val signature: String,
    val publicKey: String,
    /** b64u DER, leaf first. */
    val attestationChain: List<String>,
    val label: String,
    val uvClass: String,
    val apiLevel: Int,
) {
    override fun toString(): String = "EnrollmentClaim(challengeId=$challengeId, label=$label, uvClass=$uvClass, apiLevel=$apiLevel)"
}

/**
 * Claim answer: `claimed` with the [code] (shown only on this phone) and the
 * [claimSecret] for status polls, or for a self-claim with a ticket `done`
 * with [authenticatorState] and [readyAt].
 */
@Serializable
data class ClaimResult(
    val enrollmentId: String,
    val state: String,
    val code: String? = null,
    val codeExpiresAt: String? = null,
    val claimSecret: String? = null,
    val authenticatorState: String? = null,
    val readyAt: String? = null,
) {
    override fun toString(): String =
        "ClaimResult(enrollmentId=$enrollmentId, state=$state, authenticatorState=$authenticatorState, readyAt=$readyAt)"
}

/** `POST /auth/enrollments/status` → `{state, authenticator_state, ready_at}`. */
@Serializable
data class ClaimStatus(
    val state: EnrollmentState,
    val authenticatorState: String? = null,
    val readyAt: String? = null,
)

/** C26 `KeyDevice`. */
@Serializable
data class KeyDevice(
    val id: String,
    val deviceId: String = "",
    val label: String,
    val platform: String = "android",
    val state: KeyDeviceStatus,
    val enrolledVia: String = "",
    val uvClass: String = "",
    val createdAt: String = "",
    val activatedAt: String? = null,
    val readyAt: String? = null,
    val lastUsedAt: String? = null,
    val revokedAt: String? = null,
    val revokeReason: String? = null,
    val isCurrent: Boolean = false,
)

@Serializable
data class KeyDeviceList(val devices: List<KeyDevice> = emptyList())
