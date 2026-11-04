package io.wolftown.kaiku.domain.model

import kotlinx.serialization.Serializable

@Serializable
data class AuthResponse(
    val accessToken: String,
    val refreshToken: String? = null,
    val expiresIn: Int,
    val tokenType: String,
    val setupRequired: Boolean = false,
    /** C4: an admin-created account must choose its own password before using the API. */
    val passwordChangeRequired: Boolean = false,
    /** C27: the session this response created or refreshed. */
    val sessionId: String? = null,
    /** C27: `full` or `probation`; absent means probation. */
    val assurance: String? = null,
    /** C27: the absolute cap (90 days on Android). */
    val absoluteExpiresAt: String? = null,
) {
    override fun toString(): String =
        "AuthResponse(expiresIn=$expiresIn, sessionId=$sessionId, assurance=$assurance, absoluteExpiresAt=$absoluteExpiresAt)"
}
