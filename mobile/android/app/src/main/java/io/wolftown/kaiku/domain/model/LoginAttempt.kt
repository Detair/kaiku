package io.wolftown.kaiku.domain.model

import kotlinx.serialization.Serializable

@Serializable(with = AttemptStateSerializer::class)
enum class AttemptState(val wire: String) {
    OPEN("open"), CLAIMED("claimed"), APPROVED("approved"), CONSUMED("consumed"), DENIED("denied"),
    REPORTED("reported"), CANCELLED("cancelled"), EXPIRED("expired"), UNKNOWN("unknown"),
}

object AttemptStateSerializer : LenientEnumSerializer<AttemptState>(
    "AttemptState", AttemptState.values(), AttemptState.UNKNOWN, { it.wire },
)

@Serializable(with = LoginStepStatusSerializer::class)
enum class LoginStepStatus(val wire: String) {
    CONFIRMATION_REQUIRED("confirmation_required"), TOTP_REQUIRED("totp_required"), UNKNOWN("unknown"),
}

object LoginStepStatusSerializer : LenientEnumSerializer<LoginStepStatus>(
    "LoginStepStatus", LoginStepStatus.values(), LoginStepStatus.UNKNOWN, { it.wire },
)

/** The QR the PC shows; [payload] carries a sign-in token. */
@Serializable
data class AttemptQr(val payload: String, val expiresAt: String) {
    override fun toString(): String = "AttemptQr(expiresAt=$expiresAt)"
}

/** C27 `LoginAttempt`. */
@Serializable
data class LoginAttempt(
    val attemptId: String,
    /** `qr`, `password` or `oidc`. */
    val kind: String,
    val state: AttemptState,
    val expiresAt: String,
    val qr: AttemptQr? = null,
    val totpAvailable: Boolean = false,
)

/** C27 `LoginStep`. */
@Serializable
data class LoginStep(val status: LoginStepStatus, val attempt: LoginAttempt)
