package io.wolftown.kaiku.data.api

import io.ktor.client.HttpClient
import io.ktor.client.call.body
import io.ktor.client.request.delete
import io.ktor.client.request.get
import io.ktor.client.request.parameter
import io.ktor.client.request.post
import io.ktor.client.request.put
import io.ktor.client.request.setBody
import io.wolftown.kaiku.domain.model.AccountRequestList
import io.wolftown.kaiku.domain.model.AccountRequestSummary
import io.wolftown.kaiku.domain.model.ContactForEntry
import io.wolftown.kaiku.domain.model.ContactForList
import io.wolftown.kaiku.domain.model.CursorPage
import io.wolftown.kaiku.domain.model.MeSecurity
import io.wolftown.kaiku.domain.model.Part2Factor
import io.wolftown.kaiku.domain.model.RecoveryContact
import io.wolftown.kaiku.domain.model.RepudiateResult
import io.wolftown.kaiku.domain.model.SecurityEvent
import io.wolftown.kaiku.domain.model.SessionItem
import io.wolftown.kaiku.domain.model.SessionList
import kotlinx.serialization.Serializable
import javax.inject.Inject

/** C27/C28 own-account routes. Guarded actions need a step-up first; the grant lives on the server. */
interface SecurityApi {
    suspend fun meSecurity(): MeSecurity
    suspend fun events(before: String?, limit: Int): CursorPage<SecurityEvent>
    suspend fun repudiate(eventId: String, part2: Part2Factor?): RepudiateResult
    suspend fun cancelRequest(id: String, part2: Part2Factor?): AccountRequestSummary
    suspend fun myRequests(): List<AccountRequestSummary>
    suspend fun sessions(): List<SessionItem>
    suspend fun endSession(id: String)
    suspend fun endOtherSessions()
    suspend fun recoveryCodesLeft(): Int
    suspend fun createRecoveryCodes(): List<String>
    suspend fun setRecoveryContact(userId: String): RecoveryContact
    suspend fun removeRecoveryContact()
    suspend fun contactFor(): List<ContactForEntry>
    suspend fun confirmContactFor(userId: String)
}

@Serializable
private data class RepudiateRequest(val part2: Part2Factor?)

@Serializable
private data class CancelRequestBody(val reason: String?, val part2: Part2Factor?)

@Serializable
private data class RecoveryCodesLeft(val left: Int)

@Serializable
private data class RecoveryCodes(val recoveryCodes: List<String>) {
    override fun toString() = "RecoveryCodes(${recoveryCodes.size})"
}

@Serializable
private data class ContactRequest(val userId: String)

class SecurityApiImpl @Inject constructor(private val httpClient: HttpClient) : SecurityApi {

    override suspend fun meSecurity(): MeSecurity {
        val r = httpClient.get("/api/me/security")
        r.ensureSuccess("security overview failed")
        return r.body()
    }

    override suspend fun events(before: String?, limit: Int): CursorPage<SecurityEvent> {
        val r = httpClient.get("/api/me/security-events") {
            if (before != null) parameter("before", before)
            parameter("limit", limit.coerceIn(1, 100))
        }
        r.ensureSuccess("security log failed")
        return r.body()
    }

    override suspend fun repudiate(eventId: String, part2: Part2Factor?): RepudiateResult {
        val r = httpClient.post("/api/me/security-events/$eventId/repudiate") { setBody(RepudiateRequest(part2)) }
        r.ensureSuccess("repudiate failed")
        return r.body()
    }

    override suspend fun cancelRequest(id: String, part2: Part2Factor?): AccountRequestSummary {
        val r = httpClient.post("/api/account-requests/$id/cancel") { setBody(CancelRequestBody(null, part2)) }
        r.ensureSuccess("cancel request failed")
        return r.body()
    }

    override suspend fun myRequests(): List<AccountRequestSummary> {
        val r = httpClient.get("/api/me/account-requests")
        r.ensureSuccess("requests failed")
        return r.body<AccountRequestList>().requests
    }

    override suspend fun sessions(): List<SessionItem> {
        val r = httpClient.get("/auth/sessions")
        r.ensureSuccess("sessions failed")
        return r.body<SessionList>().sessions
    }

    override suspend fun endSession(id: String) {
        httpClient.delete("/auth/sessions/$id").ensureSuccess("end session failed")
    }

    override suspend fun endOtherSessions() {
        httpClient.delete("/auth/sessions").ensureSuccess("end other sessions failed")
    }

    override suspend fun recoveryCodesLeft(): Int {
        val r = httpClient.get("/auth/recovery-codes/count")
        r.ensureSuccess("recovery code count failed")
        return r.body<RecoveryCodesLeft>().left
    }

    override suspend fun createRecoveryCodes(): List<String> {
        val r = httpClient.post("/auth/recovery-codes")
        r.ensureSuccess("recovery codes failed")
        return r.body<RecoveryCodes>().recoveryCodes
    }

    override suspend fun setRecoveryContact(userId: String): RecoveryContact {
        val r = httpClient.put("/api/me/recovery-contact") { setBody(ContactRequest(userId)) }
        r.ensureSuccess("recovery contact failed")
        return r.body()
    }

    override suspend fun removeRecoveryContact() {
        httpClient.delete("/api/me/recovery-contact").ensureSuccess("remove recovery contact failed")
    }

    override suspend fun contactFor(): List<ContactForEntry> {
        val r = httpClient.get("/api/me/recovery-contact-for")
        r.ensureSuccess("contact-for failed")
        return r.body<ContactForList>().accounts
    }

    override suspend fun confirmContactFor(userId: String) {
        httpClient.post("/api/me/recovery-contact-for/$userId/confirm").ensureSuccess("confirm contact failed")
    }
}
