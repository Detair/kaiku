package io.wolftown.kaiku.domain.model

import kotlinx.serialization.Serializable

/** Presence states. Values from a newer server decode to [UNKNOWN]. */
@Serializable(with = UserStatusSerializer::class)
enum class UserStatus {
    ONLINE,
    IDLE,
    DND,
    OFFLINE,
    UNKNOWN;
}

object UserStatusSerializer : LenientEnumSerializer<UserStatus>(
    serialName = "UserStatus",
    values = UserStatus.values(),
    unknown = UserStatus.UNKNOWN,
    wireName = { status ->
        when (status) {
            UserStatus.ONLINE -> "online"
            UserStatus.IDLE -> "away"
            UserStatus.DND -> "busy"
            UserStatus.OFFLINE -> "offline"
            UserStatus.UNKNOWN -> "unknown"
        }
    },
)

@Serializable
data class User(
    val id: String,
    val username: String,
    val displayName: String,
    val avatarUrl: String? = null,
    val status: UserStatus = UserStatus.OFFLINE,
    val mfaEnabled: Boolean = false,
    val createdAt: String = "",
    /** C4: `GET /auth/me` reports a pending forced password change. */
    val passwordChangeRequired: Boolean = false
)
