package io.wolftown.kaiku.data.api

import io.ktor.client.HttpClient
import io.ktor.client.call.body
import io.ktor.client.request.delete
import io.ktor.client.request.get
import io.ktor.client.request.header
import io.ktor.client.request.post
import io.ktor.client.request.setBody
import io.wolftown.kaiku.domain.model.ClaimResult
import io.wolftown.kaiku.domain.model.ClaimStatus
import io.wolftown.kaiku.domain.model.Enrollment
import io.wolftown.kaiku.domain.model.EnrollmentClaim
import io.wolftown.kaiku.domain.model.EnrollmentIssued
import io.wolftown.kaiku.domain.model.EnrollmentOpen
import io.wolftown.kaiku.domain.model.PopChallenge
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonObject
import javax.inject.Inject

/**
 * C26 enrolment core (SM2). Issuer routes use the bearer (`own`, `admin`) or the
 * registration ticket. The new-device routes are public; [origin] sends them to
 * a server that is not (yet) configured, for an unconfigured app opened with a
 * setup code (null = the configured server). They never carry a bearer.
 */
interface EnrollmentApi {
    suspend fun issueOwn(): EnrollmentIssued
    suspend fun issueWithTicket(ticket: String): EnrollmentIssued
    suspend fun get(id: String): Enrollment
    suspend fun cancel(id: String)
    suspend fun open(setupToken: String, origin: String? = null): EnrollmentOpen
    suspend fun popChallenge(setupToken: String, deviceId: String, publicKeyB64u: String, origin: String? = null): PopChallenge
    suspend fun claim(req: EnrollmentClaim, ticket: String? = null, origin: String? = null): ClaimResult
    suspend fun status(enrollmentId: String, claimSecret: String, waitSeconds: Int, origin: String? = null): ClaimStatus
}

const val TICKET_HEADER = "X-Kaiku-Ticket"

@Serializable
private data class OpenRequest(val setupToken: String) {
    override fun toString() = "OpenRequest"
}

@Serializable
private data class PopChallengeRequest(val setupToken: String, val deviceId: String, val publicKey: String) {
    override fun toString() = "PopChallengeRequest(deviceId=$deviceId)"
}

@Serializable
private data class StatusRequest(val enrollmentId: String, val claimSecret: String, val wait: Int) {
    override fun toString() = "StatusRequest(enrollmentId=$enrollmentId, wait=$wait)"
}

class EnrollmentApiImpl @Inject constructor(private val httpClient: HttpClient) : EnrollmentApi {

    override suspend fun issueOwn(): EnrollmentIssued {
        val r = httpClient.post("/auth/enrollments") { setBody(JsonObject(emptyMap())) }
        r.ensureSuccess("issue failed")
        return r.body()
    }

    override suspend fun issueWithTicket(ticket: String): EnrollmentIssued {
        val r = httpClient.post("/auth/registration/enrollments") {
            publicRoute()
            header(TICKET_HEADER, ticket)
            setBody(JsonObject(emptyMap()))
        }
        r.ensureSuccess("issue with ticket failed")
        return r.body()
    }

    override suspend fun get(id: String): Enrollment {
        val r = httpClient.get("/auth/enrollments/$id")
        r.ensureSuccess("enrolment failed")
        return r.body()
    }

    override suspend fun cancel(id: String) {
        httpClient.delete("/auth/enrollments/$id").ensureSuccess("cancel failed")
    }

    override suspend fun open(setupToken: String, origin: String?): EnrollmentOpen {
        val r = httpClient.post(at(origin, "/auth/enrollments/open")) {
            publicRoute()
            setBody(OpenRequest(setupToken))
        }
        r.ensureSuccess("open failed")
        return r.body()
    }

    override suspend fun popChallenge(setupToken: String, deviceId: String, publicKeyB64u: String, origin: String?): PopChallenge {
        val r = httpClient.post(at(origin, "/auth/enrollments/pop-challenge")) {
            publicRoute()
            setBody(PopChallengeRequest(setupToken, deviceId, publicKeyB64u))
        }
        r.ensureSuccess("pop challenge failed")
        return r.body()
    }

    override suspend fun claim(req: EnrollmentClaim, ticket: String?, origin: String?): ClaimResult {
        val r = httpClient.post(at(origin, "/auth/enrollments/claim")) {
            publicRoute()
            if (ticket != null) header(TICKET_HEADER, ticket)
            setBody(req)
        }
        r.ensureSuccess("claim failed")
        return r.body()
    }

    override suspend fun status(enrollmentId: String, claimSecret: String, waitSeconds: Int, origin: String?): ClaimStatus {
        val r = httpClient.post(at(origin, "/auth/enrollments/status")) {
            publicRoute()
            setBody(StatusRequest(enrollmentId, claimSecret, waitSeconds.coerceIn(0, 25)))
        }
        r.ensureSuccess("status failed")
        return r.body()
    }
}
