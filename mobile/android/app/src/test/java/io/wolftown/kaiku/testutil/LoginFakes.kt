package io.wolftown.kaiku.testutil

import io.ktor.http.HttpStatusCode
import io.wolftown.kaiku.data.api.ApiErrorFields
import io.wolftown.kaiku.data.api.ApiException
import io.wolftown.kaiku.data.api.ApprovalApi
import io.wolftown.kaiku.data.keydevice.KeyDeviceStore
import io.wolftown.kaiku.data.keydevice.StoredKeyDevice
import io.wolftown.kaiku.domain.model.AnswerResult
import io.wolftown.kaiku.domain.model.ApprovalChallenge
import io.wolftown.kaiku.domain.model.AuthResponse
import io.wolftown.kaiku.domain.model.KeyDevice
import io.wolftown.kaiku.domain.model.KeyDeviceStatus

fun apiError(status: Int, code: String, fields: ApiErrorFields = ApiErrorFields()) =
    ApiException(HttpStatusCode.fromValue(status), code, code = code, fields = fields)

/** In-memory key-device store; survives "new repository instances" when shared. */
class InMemoryKeyDeviceStore : KeyDeviceStore {
    var stored: StoredKeyDevice? = null
    override fun load(): StoredKeyDevice? = stored
    override fun save(s: StoredKeyDevice) { stored = s }
    override fun clear() { stored = null }
}

/** Scripted [ApprovalApi]: every call is recorded; set the next answers or failures. */
class FakeApprovalApi : ApprovalApi {
    val calls = mutableListOf<String>()
    var nextChallenge: ApprovalChallenge? = null
    var failWith: Exception? = null
    var answerResult: AnswerResult = AnswerResult(state = "approved")
    var answerFailure: Exception? = null
    var deviceLoginResult: AuthResponse = AuthResponse(
        accessToken = "new-access", refreshToken = "new-refresh", expiresIn = 900, tokenType = "Bearer",
        sessionId = "77777777-7777-4777-8777-777777777777", assurance = "full", absoluteExpiresAt = "2027-02-01T12:00:00Z",
    )
    var deviceLoginFailure: Exception? = null
    var lastAnswer: Triple<String, String, String>? = null
    var devices: List<KeyDevice> = emptyList()

    private fun challenge(call: String): ApprovalChallenge {
        calls += call
        failWith?.let { throw it }
        return nextChallenge ?: error("no challenge scripted for $call")
    }

    override suspend fun claim(tokenB64u: String) = challenge("claim")
    override suspend fun stepUp(action: String, target: String) = challenge("stepUp:$action:$target")
    override suspend fun enrollmentChallenge(enrollmentId: String) = challenge("enrollmentChallenge:$enrollmentId")

    override suspend fun answer(challengeId: String, decision: String, responseB64u: String, signatureB64u: String): AnswerResult {
        calls += "answer:$challengeId:$decision"
        lastAnswer = Triple(decision, responseB64u, signatureB64u)
        answerFailure?.let { throw it }
        return answerResult
    }

    override suspend fun deny(challengeId: String) { calls += "deny:$challengeId" }
    override suspend fun release(challengeId: String) { calls += "release:$challengeId" }
    override suspend fun deviceLoginChallenge(userId: String, deviceId: String) = challenge("deviceLoginChallenge:$userId:$deviceId")

    override suspend fun deviceLogin(challengeId: String, signatureB64u: String): AuthResponse {
        calls += "deviceLogin:$challengeId"
        deviceLoginFailure?.let { throw it }
        return deviceLoginResult
    }

    override suspend fun keyDevices(): List<KeyDevice> { calls += "keyDevices"; return devices }

    override suspend fun removeKeyDevice(id: String): KeyDevice {
        calls += "removeKeyDevice:$id"
        failWith?.let { throw it }
        return KeyDevice(id = id, label = "Pixel 8", state = KeyDeviceStatus.REVOKED)
    }

    override suspend fun reportInvalidated(id: String): KeyDevice {
        calls += "reportInvalidated:$id"
        return KeyDevice(id = id, label = "Pixel 8", state = KeyDeviceStatus.REVOKED)
    }
}
