package io.wolftown.kaiku.data.api

import io.ktor.client.HttpClient
import io.ktor.client.call.body
import io.ktor.client.request.HttpRequestBuilder
import io.ktor.client.request.delete
import io.ktor.client.request.get
import io.ktor.client.request.headers
import io.ktor.client.request.post
import io.ktor.client.request.setBody
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.wolftown.kaiku.domain.model.AnswerResult
import io.wolftown.kaiku.domain.model.ApprovalChallenge
import io.wolftown.kaiku.domain.model.KeyDevice
import io.wolftown.kaiku.domain.model.KeyDeviceList
import io.wolftown.kaiku.domain.model.User
import kotlinx.serialization.Serializable
import javax.inject.Inject

/**
 * The calls a signed-out key phone makes to remove itself ("Use another account"), with
 * credentials that exist only inside that one operation. Every call names its server ([origin],
 * absolute) and its bearer ([access]); the auth interceptor is skipped, so no stored token is
 * attached, nothing is refreshed and nothing is stored.
 */
interface RemovalApi {
    suspend fun me(origin: String, access: String): User
    suspend fun keyDevices(origin: String, access: String): List<KeyDevice>
    suspend fun stepUp(origin: String, access: String, action: String, target: String): ApprovalChallenge
    suspend fun answer(origin: String, access: String, challengeId: String, decision: String, responseB64u: String, signatureB64u: String): AnswerResult
    /** The server's answer to the removal: the key device as it is afterwards. */
    suspend fun removeKeyDevice(origin: String, access: String, id: String): KeyDevice
}

@Serializable
private data class RemovalStepUpRequest(val action: String, val target: String)

@Serializable
private data class RemovalAnswerRequest(val decision: String, val response: String, val signature: String) {
    override fun toString() = "RemovalAnswerRequest(decision=$decision)"
}

class RemovalApiImpl @Inject constructor(private val httpClient: HttpClient) : RemovalApi {

    private fun HttpRequestBuilder.with(access: String) {
        attributes.put(KaikuHttpClient.SkipAuthInterceptor, true)
        headers { set(HttpHeaders.Authorization, "Bearer $access") }
    }

    private fun url(origin: String, path: String): String = origin.trimEnd('/') + path

    override suspend fun me(origin: String, access: String): User {
        val r = httpClient.get(url(origin, "/auth/me")) { with(access) }
        r.ensureExactly(HttpStatusCode.OK, "account failed")
        return r.body()
    }

    override suspend fun keyDevices(origin: String, access: String): List<KeyDevice> {
        val r = httpClient.get(url(origin, "/api/approval-devices")) { with(access) }
        r.ensureExactly(HttpStatusCode.OK, "key devices failed")
        return r.body<KeyDeviceList>().devices
    }

    override suspend fun stepUp(origin: String, access: String, action: String, target: String): ApprovalChallenge {
        val r = httpClient.post(url(origin, "/api/approvals/step-up")) {
            with(access)
            setBody(RemovalStepUpRequest(action, target))
        }
        r.ensureExactly(HttpStatusCode.Created, "step-up challenge failed")
        return r.body()
    }

    override suspend fun answer(
        origin: String,
        access: String,
        challengeId: String,
        decision: String,
        responseB64u: String,
        signatureB64u: String,
    ): AnswerResult {
        val r = httpClient.post(url(origin, "/api/approvals/$challengeId/answer")) {
            with(access)
            setBody(RemovalAnswerRequest(decision, responseB64u, signatureB64u))
        }
        r.ensureExactly(HttpStatusCode.OK, "answer failed")
        return r.body()
    }

    override suspend fun removeKeyDevice(origin: String, access: String, id: String): KeyDevice {
        val r = httpClient.delete(url(origin, "/api/approval-devices/$id")) { with(access) }
        r.ensureExactly(HttpStatusCode.OK, "remove key device failed")
        return r.body()
    }
}
