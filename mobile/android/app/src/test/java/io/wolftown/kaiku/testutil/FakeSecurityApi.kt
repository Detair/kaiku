package io.wolftown.kaiku.testutil

import io.wolftown.kaiku.data.api.SecurityApi
import io.wolftown.kaiku.domain.model.AccountRequestSummary
import io.wolftown.kaiku.domain.model.ContactForEntry
import io.wolftown.kaiku.domain.model.CursorPage
import io.wolftown.kaiku.domain.model.MeSecurity
import io.wolftown.kaiku.domain.model.Part2Factor
import io.wolftown.kaiku.domain.model.RecoveryContact
import io.wolftown.kaiku.domain.model.RepudiateResult
import io.wolftown.kaiku.domain.model.SecurityEvent
import io.wolftown.kaiku.domain.model.SecuritySession
import io.wolftown.kaiku.domain.model.SessionItem

/** Scripted [SecurityApi]; every call is recorded. */
class FakeSecurityApi : SecurityApi {
    val calls = mutableListOf<String>()
    var me = MeSecurity(session = SecuritySession("full", listOf("device_key")), recoveryCodesLeft = 9, keyDevices = 2, mayApprove = true)
    var sessions: List<SessionItem> = emptyList()
    var pages = ArrayDeque<CursorPage<SecurityEvent>>()
    var failure: Exception? = null
    var repudiateResult: RepudiateResult? = null
    var requests: List<AccountRequestSummary> = emptyList()
    var lastPart2: Part2Factor? = null
    var cancelResult: AccountRequestSummary? = null

    private fun call(name: String) {
        calls += name
        failure?.let { throw it }
    }

    override suspend fun meSecurity(): MeSecurity { calls += "meSecurity"; return me }
    override suspend fun events(before: String?, limit: Int): CursorPage<SecurityEvent> {
        calls += "events:${before ?: ""}"
        return pages.removeFirstOrNull() ?: CursorPage(emptyList())
    }
    override suspend fun repudiate(eventId: String, part2: Part2Factor?): RepudiateResult {
        call("repudiate:$eventId"); lastPart2 = part2; return repudiateResult!!
    }
    override suspend fun cancelRequest(id: String, part2: Part2Factor?): AccountRequestSummary {
        call("cancelRequest:$id"); lastPart2 = part2; return cancelResult!!
    }
    override suspend fun myRequests(): List<AccountRequestSummary> { calls += "myRequests"; return requests }
    override suspend fun sessions(): List<SessionItem> { calls += "sessions"; return sessions }
    override suspend fun endSession(id: String) = call("endSession:$id")
    override suspend fun endOtherSessions() = call("endOtherSessions")
    override suspend fun recoveryCodesLeft(): Int { calls += "recoveryCodesLeft"; return me.recoveryCodesLeft }
    override suspend fun createRecoveryCodes(): List<String> { call("createRecoveryCodes"); return listOf("AAAA-BBBB-CCCC-D", "EEEE-FFFF-GGGG-H") }
    override suspend fun setRecoveryContact(userId: String): RecoveryContact {
        call("setRecoveryContact:$userId"); return RecoveryContact(userId, "oma", "Oma")
    }
    override suspend fun removeRecoveryContact() = call("removeRecoveryContact")
    override suspend fun contactFor(): List<ContactForEntry> { calls += "contactFor"; return emptyList() }
    override suspend fun confirmContactFor(userId: String) = call("confirmContactFor:$userId")
}
