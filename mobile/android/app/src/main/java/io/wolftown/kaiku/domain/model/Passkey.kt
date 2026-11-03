package io.wolftown.kaiku.domain.model

import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonObject

/** C29 `{ceremony_id, options}`; `options` = `{"publicKey": …}` (WebAuthn L3 JSON). */
@Serializable
data class CeremonyStart(
    val ceremonyId: String,
    val options: JsonObject,
)

/** C29 `Passkey`. Unknown states map to `unknown` (C5). */
@Serializable
data class Passkey(
    val id: String,
    val label: String,
    val state: String,
    val createdAt: String,
    val lastUsedAt: String? = null,
    val readyAt: String? = null,
    val backupEligible: Boolean = false,
    val backupState: Boolean = false,
    val transports: List<String> = emptyList(),
    val aaguid: String? = null,
) {
    val isActive: Boolean get() = state == "active"
    val isPending: Boolean get() = state == "pending"
}

/** `GET /auth/passkeys`. */
@Serializable
data class PasskeyList(
    val rpId: String,
    val userHandle: String,
    val passkeys: List<Passkey> = emptyList(),
)

/** `DELETE /auth/passkeys/{id}`: the data for the credential signals. */
@Serializable
data class PasskeyRemoved(
    val rpId: String,
    val userHandle: String,
    val credentialId: String,
    val remainingCredentialIds: List<String> = emptyList(),
)

/** `GET /auth/reauth/methods` (C27 v1.0). */
@Serializable
data class ReauthMethods(
    val passkey: Boolean = false,
    val device: Boolean = false,
)
