package io.wolftown.kaiku.domain.model

import kotlinx.serialization.Serializable

/** `GET /api/admin/users` item. */
@Serializable
data class AdminUserSummary(
    val id: String,
    val username: String,
    val displayName: String = "",
    val isBanned: Boolean = false,
)

@Serializable
data class AdminUserPage(val items: List<AdminUserSummary> = emptyList(), val total: Long = 0)

@Serializable
data class OpenSessions(val full: Int = 0, val probation: Int = 0)

/**
 * `GET /api/admin/users/{id}/details` with the C27 fields ("Admins see per
 * user"). [isAdmin] is read when the server sends it; otherwise the server's
 * 403 `ADMIN_TARGET_IS_ADMIN` still stops immediate actions on admins.
 */
@Serializable
data class AdminUserDetail(
    val id: String,
    val username: String,
    val displayName: String = "",
    val isAdmin: Boolean = false,
    val mayApprove: Boolean = false,
    val keyDevices: List<AuthenticatorRef> = emptyList(),
    val passkeys: List<AuthenticatorRef> = emptyList(),
    val passwordState: String? = null,
    val totpState: String? = null,
    val recoveryCodesLeft: Int = 0,
    val bootstrapOpen: Boolean = false,
    val recoveryContact: RecoveryContact? = null,
    val openRequest: AccountRequestSummary? = null,
    val openSessions: OpenSessions? = null,
) {
    /** B.0: an account without any sign-in method yet is active at once after setup. */
    val isNewAccount: Boolean
        get() = keyDevices.isEmpty() && passkeys.isEmpty() && passwordState == null && totpState == null
}
