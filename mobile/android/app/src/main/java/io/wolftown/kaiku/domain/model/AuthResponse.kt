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
    val passwordChangeRequired: Boolean = false
)
