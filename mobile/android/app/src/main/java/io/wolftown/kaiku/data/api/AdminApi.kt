package io.wolftown.kaiku.data.api

import io.ktor.client.HttpClient
import io.ktor.client.call.body
import io.ktor.client.request.get
import io.ktor.client.request.parameter
import io.ktor.client.request.post
import io.ktor.client.request.put
import io.ktor.client.request.setBody
import io.wolftown.kaiku.domain.model.AdminUserDetail
import io.wolftown.kaiku.domain.model.AdminUserPage
import io.wolftown.kaiku.domain.model.AdminUserSummary
import io.wolftown.kaiku.domain.model.EnrollmentIssued
import kotlinx.serialization.Serializable
import javax.inject.Inject

/**
 * Admin routes of the v1.0 login used on the admin's own key phone (Task B13;
 * coordinator call 1: setup codes only from there). Guarded actions need the
 * matching device step-up first; the admin routes need a full session.
 */
interface AdminApi {
    suspend fun status(): Boolean
    suspend fun elevate(reason: String?)
    suspend fun users(search: String?): List<AdminUserSummary>
    suspend fun user(id: String): AdminUserDetail
    /** C4/C23 amended by C27: no `temporary_password`; the account is passwordless. */
    suspend fun createUser(username: String, displayName: String, recoveryContactId: String?): AdminUserSummary
    suspend fun issueEnrollment(userId: String, mode: String, claimantMethod: String, note: String, lostDeviceIds: List<String>): EnrollmentIssued
    suspend fun setMayApprove(userId: String, value: Boolean)
    /** "Remove device now" (C26, L1b): another account's key device or passkey; needs `admin.authenticator.remove`. */
    suspend fun removeAuthenticator(userId: String, authenticatorId: String)
}

@Serializable
private data class AdminStatus(val isAdmin: Boolean = false)

@Serializable
private data class ElevateRequest(val reason: String?)

@Serializable
private data class CreateUserBody(val username: String, val displayName: String, val recoveryContactId: String?)

@Serializable
private data class ClaimantCheckBody(val method: String, val note: String)

@Serializable
private data class IssueBody(val mode: String, val claimantCheck: ClaimantCheckBody, val lostDeviceIds: List<String>)

@Serializable
private data class MayApproveBody(val mayApprove: Boolean)

class AdminApiImpl @Inject constructor(private val httpClient: HttpClient) : AdminApi {

    override suspend fun status(): Boolean {
        val r = httpClient.get("/api/admin/status")
        r.ensureSuccess("admin status failed")
        return r.body<AdminStatus>().isAdmin
    }

    override suspend fun elevate(reason: String?) {
        httpClient.post("/api/admin/elevate") { setBody(ElevateRequest(reason)) }.ensureSuccess("elevate failed")
    }

    override suspend fun users(search: String?): List<AdminUserSummary> {
        val r = httpClient.get("/api/admin/users") {
            if (!search.isNullOrBlank()) parameter("search", search.trim())
            parameter("limit", 100)
        }
        r.ensureSuccess("users failed")
        return r.body<AdminUserPage>().items
    }

    override suspend fun user(id: String): AdminUserDetail {
        val r = httpClient.get("/api/admin/users/$id/details")
        r.ensureSuccess("user failed")
        return r.body()
    }

    override suspend fun createUser(username: String, displayName: String, recoveryContactId: String?): AdminUserSummary {
        val r = httpClient.post("/api/admin/users") { setBody(CreateUserBody(username, displayName, recoveryContactId)) }
        r.ensureSuccess("create user failed")
        return r.body()
    }

    override suspend fun issueEnrollment(
        userId: String,
        mode: String,
        claimantMethod: String,
        note: String,
        lostDeviceIds: List<String>,
    ): EnrollmentIssued {
        val r = httpClient.post("/api/admin/users/$userId/enrollments") {
            setBody(IssueBody(mode, ClaimantCheckBody(claimantMethod, note), lostDeviceIds))
        }
        r.ensureSuccess("admin enrolment failed")
        return r.body()
    }

    override suspend fun setMayApprove(userId: String, value: Boolean) {
        httpClient.put("/api/admin/users/$userId/may-approve") { setBody(MayApproveBody(value)) }.ensureSuccess("may approve failed")
    }

    override suspend fun removeAuthenticator(userId: String, authenticatorId: String) {
        httpClient.post("/api/admin/users/$userId/authenticators/$authenticatorId/remove").ensureSuccess("remove failed")
    }
}
