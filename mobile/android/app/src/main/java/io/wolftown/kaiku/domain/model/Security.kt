package io.wolftown.kaiku.domain.model

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonObject

/** C27 `GET /api/me/security`. Unknown prompts are kept as strings and ignored by the screens. */
@Serializable
data class MeSecurity(
    val session: SecuritySession? = null,
    val password: FactorState? = null,
    val totp: FactorState? = null,
    val recoveryCodesLeft: Int = 0,
    val keyDevices: Int = 0,
    val passkeys: Int = 0,
    val mayApprove: Boolean = false,
    val bootstrapOpen: Boolean = false,
    val recoveryContact: RecoveryContact? = null,
    val openRequest: AccountRequestSummary? = null,
    val prompts: List<String> = emptyList(),
)

@Serializable
data class SecuritySession(val assurance: String = "probation", val authMethods: List<String> = emptyList())

/** `active`, `throttled` or `compromised`. */
@Serializable
data class FactorState(val state: String)

/** C28 `RecoveryContact`. */
@Serializable
data class RecoveryContact(
    val userId: String,
    val username: String,
    val displayName: String = "",
    val namedAt: String = "",
    val confirmedAt: String? = null,
    val countsFrom: String? = null,
)

/** `GET /api/me/recovery-contact-for` entry: an account that named me. */
@Serializable
data class ContactForEntry(
    val userId: String,
    val username: String,
    val displayName: String = "",
    val namedAt: String = "",
    val confirmedAt: String? = null,
)

@Serializable
data class ContactForList(val accounts: List<ContactForEntry> = emptyList())

@Serializable(with = NoticeActionSerializer::class)
enum class NoticeAction(val wire: String) {
    REPUDIATE("repudiate"), CANCEL_REQUEST("cancel_request"), REMOVE_DEVICE("remove_device"),
    CONFIRM_CONTACT("confirm_contact"), UNKNOWN("unknown"),
}

object NoticeActionSerializer : LenientEnumSerializer<NoticeAction>(
    "NoticeAction", NoticeAction.values(), NoticeAction.UNKNOWN, { it.wire },
)

/** C28 `Action`, computed for the reader; [warning] must be shown first. */
@Serializable
data class EventAction(val action: NoticeAction, val via: String? = null, val warning: String? = null)

@Serializable
data class EventActor(val kind: String, val userId: String? = null, val username: String? = null)

@Serializable
data class EventClient(@SerialName("class") val clientClass: String = "", val label: String = "")

@Serializable
data class AuthenticatorRef(val id: String, val type: String = "", val label: String = "", val state: String = "")

/** C28 `SecurityEvent`. */
@Serializable
data class SecurityEvent(
    val id: String,
    val eventType: String,
    val severity: String = "normal",
    val createdAt: String,
    val subjectId: String = "",
    val actor: EventActor? = null,
    val client: EventClient? = null,
    val place: String? = null,
    val ip: String? = null,
    val sessionId: String? = null,
    val authenticator: AuthenticatorRef? = null,
    val requestId: String? = null,
    val details: JsonObject = JsonObject(emptyMap()),
    val actions: List<EventAction> = emptyList(),
    val repudiatedAt: String? = null,
)

/** C28 `SecurityNotice` (the Kaiku DM message payload). */
@Serializable
data class SecurityNotice(
    val noticeId: String,
    val subject: AccountRef,
    val eventType: String,
    val severity: String = "normal",
    val count: Int = 1,
    val firstAt: String = "",
    val lastAt: String = "",
    val events: List<SecurityEvent> = emptyList(),
    val eventsTruncated: Boolean = false,
)

/** `repudiate` → what "That wasn't me" ended, revoked, marked compromised and voided. */
@Serializable
data class RepudiateResult(
    val event: SecurityEvent,
    val endedSessionIds: List<String> = emptyList(),
    val revokedAuthenticatorIds: List<String> = emptyList(),
    val compromised: List<String> = emptyList(),
    val voidedRequestIds: List<String> = emptyList(),
)

/** Owner rule part 2: a fresh TOTP or recovery code (B.2). */
@Serializable
data class Part2Factor(val factor: Factor) {
    @Serializable
    data class Factor(val type: String, val code: String) {
        override fun toString(): String = "Factor(type=$type)"
    }

    override fun toString(): String = "Part2Factor(${factor.type})"

    companion object {
        fun totp(code: String) = Part2Factor(Factor("totp", code))
        fun recoveryCode(code: String) = Part2Factor(Factor("recovery_code", code))
    }
}

@Serializable
data class DeviceRef(val id: String, val label: String = "")

@Serializable
data class StartedBy(val userId: String, val username: String)

@Serializable
data class NewAuthenticator(val type: String, val label: String = "")

@Serializable
data class CancelVia(val via: String)

/** C27 `AccountRequestSummary`. */
@Serializable
data class AccountRequestSummary(
    val id: String,
    val subject: AccountRef,
    val kind: String = "",
    val purpose: String = "",
    val state: String,
    val createdAt: String = "",
    val readyAt: String = "",
    val startedBy: StartedBy? = null,
    val lostDevices: List<DeviceRef> = emptyList(),
    val newAuthenticator: NewAuthenticator? = null,
    val cancel: CancelVia? = null,
)

@Serializable
data class AccountRequestList(val requests: List<AccountRequestSummary> = emptyList())

/** `GET /auth/sessions` item with the C27 fields. */
@Serializable
data class SessionItem(
    val id: String,
    val device: String = "",
    val createdAt: String = "",
    val expiresAt: String = "",
    val isCurrent: Boolean = false,
    val assurance: String = "probation",
    val clientClass: String = "",
    val authMethods: List<String> = emptyList(),
    val authenticator: AuthenticatorRef? = null,
    val lastUsedAt: String? = null,
    val absoluteExpiresAt: String? = null,
)

@Serializable
data class SessionList(val sessions: List<SessionItem> = emptyList())
