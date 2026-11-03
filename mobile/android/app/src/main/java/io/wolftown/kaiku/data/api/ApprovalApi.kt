package io.wolftown.kaiku.data.api

import io.ktor.client.HttpClient
import io.ktor.client.call.body
import io.ktor.client.request.delete
import io.ktor.client.request.get
import io.ktor.client.request.post
import io.ktor.client.request.setBody
import io.wolftown.kaiku.domain.model.AnswerResult
import io.wolftown.kaiku.domain.model.ApprovalChallenge
import io.wolftown.kaiku.domain.model.AuthResponse
import io.wolftown.kaiku.domain.model.KeyDevice
import io.wolftown.kaiku.domain.model.KeyDeviceList
import kotlinx.serialization.Serializable
import javax.inject.Inject

/** C26 approvals, device-key sign-in and key devices. */
interface ApprovalApi {
    suspend fun claim(tokenB64u: String): ApprovalChallenge
    suspend fun stepUp(action: String, target: String): ApprovalChallenge
    suspend fun enrollmentChallenge(enrollmentId: String): ApprovalChallenge
    suspend fun answer(challengeId: String, decision: String, responseB64u: String, signatureB64u: String): AnswerResult
    suspend fun deny(challengeId: String)
    suspend fun release(challengeId: String)
    /** [origin]: the key's own server (absolute URL), never another configured one; null = configured. */
    suspend fun deviceLoginChallenge(userId: String, deviceId: String, origin: String? = null): ApprovalChallenge
    suspend fun deviceLogin(challengeId: String, signatureB64u: String): AuthResponse
    suspend fun keyDevices(): List<KeyDevice>
    suspend fun removeKeyDevice(id: String): KeyDevice
    suspend fun reportInvalidated(id: String): KeyDevice
}

@Serializable
private data class ClaimRequest(val token: String) {
    override fun toString() = "ClaimRequest"
}

@Serializable
private data class StepUpRequest(val action: String, val target: String)

@Serializable
private data class AnswerRequest(val decision: String, val response: String, val signature: String) {
    override fun toString() = "AnswerRequest(decision=$decision)"
}

@Serializable
private data class DeviceLoginChallengeRequest(val userId: String, val deviceId: String)

@Serializable
private data class DeviceLoginRequest(val challengeId: String, val signature: String) {
    override fun toString() = "DeviceLoginRequest(challengeId=$challengeId)"
}

class ApprovalApiImpl @Inject constructor(private val httpClient: HttpClient) : ApprovalApi {

    override suspend fun claim(tokenB64u: String): ApprovalChallenge {
        val r = httpClient.post("/api/approvals/claim") { setBody(ClaimRequest(tokenB64u)) }
        r.ensureSuccess("claim failed")
        return r.body()
    }

    override suspend fun stepUp(action: String, target: String): ApprovalChallenge {
        val r = httpClient.post("/api/approvals/step-up") { setBody(StepUpRequest(action, target)) }
        r.ensureSuccess("step-up challenge failed")
        return r.body()
    }

    override suspend fun enrollmentChallenge(enrollmentId: String): ApprovalChallenge {
        val r = httpClient.post("/auth/enrollments/$enrollmentId/challenge")
        r.ensureSuccess("enrolment challenge failed")
        return r.body()
    }

    override suspend fun answer(
        challengeId: String,
        decision: String,
        responseB64u: String,
        signatureB64u: String,
    ): AnswerResult {
        val r = httpClient.post("/api/approvals/$challengeId/answer") {
            setBody(AnswerRequest(decision, responseB64u, signatureB64u))
        }
        r.ensureSuccess("answer failed")
        return r.body()
    }

    override suspend fun deny(challengeId: String) {
        httpClient.post("/api/approvals/$challengeId/deny").ensureSuccess("deny failed")
    }

    override suspend fun release(challengeId: String) {
        httpClient.post("/api/approvals/$challengeId/release").ensureSuccess("release failed")
    }

    override suspend fun deviceLoginChallenge(userId: String, deviceId: String, origin: String?): ApprovalChallenge {
        val r = httpClient.post(at(origin, "/auth/device-login/challenge")) {
            publicRoute()
            setBody(DeviceLoginChallengeRequest(userId, deviceId))
        }
        r.ensureSuccess("device-login challenge failed")
        return r.body()
    }

    override suspend fun deviceLogin(challengeId: String, signatureB64u: String): AuthResponse {
        val r = httpClient.post("/auth/device-login") {
            publicRoute()
            setBody(DeviceLoginRequest(challengeId, signatureB64u))
        }
        r.ensureSuccess("device login failed")
        return r.body()
    }

    override suspend fun keyDevices(): List<KeyDevice> {
        val r = httpClient.get("/api/approval-devices")
        r.ensureSuccess("key devices failed")
        return r.body<KeyDeviceList>().devices
    }

    override suspend fun removeKeyDevice(id: String): KeyDevice {
        val r = httpClient.delete("/api/approval-devices/$id")
        r.ensureSuccess("remove key device failed")
        return r.body()
    }

    override suspend fun reportInvalidated(id: String): KeyDevice {
        val r = httpClient.post("/api/approval-devices/$id/invalidated")
        r.ensureSuccess("report invalidated failed")
        return r.body()
    }
}
