package io.wolftown.kaiku.data.api

import io.ktor.client.HttpClient
import io.ktor.client.call.body
import io.ktor.client.request.HttpRequestBuilder
import io.ktor.client.request.delete
import io.ktor.client.request.get
import io.ktor.client.request.header
import io.ktor.client.request.patch
import io.ktor.client.request.post
import io.ktor.client.request.setBody
import io.ktor.client.statement.HttpResponse
import io.ktor.client.statement.bodyAsText
import io.ktor.http.encodeURLPathPart
import io.ktor.http.isSuccess
import io.wolftown.kaiku.data.KaikuJson
import io.wolftown.kaiku.domain.model.AuthResponse
import io.wolftown.kaiku.domain.model.CeremonyStart
import io.wolftown.kaiku.domain.model.Passkey
import io.wolftown.kaiku.domain.model.PasskeyList
import io.wolftown.kaiku.domain.model.PasskeyRemoved
import io.wolftown.kaiku.domain.model.ReauthMethods
import io.wolftown.kaiku.domain.model.StepUpGrant
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonObject
import javax.inject.Inject

/** The C29 passkey routes; every call sends `X-Kaiku-Client: android`. */
interface PasskeyApi {
    suspend fun loginStart(): CeremonyStart
    suspend fun loginFinish(ceremonyId: String, credential: JsonObject, username: String?): AuthResponse
    suspend fun stepUpStart(action: String, target: String): CeremonyStart
    suspend fun stepUpFinish(ceremonyId: String, credential: JsonObject): StepUpGrant
    suspend fun registerStart(label: String?): CeremonyStart
    suspend fun registerFinish(ceremonyId: String, credential: JsonObject): Passkey
    suspend fun list(): PasskeyList
    suspend fun rename(id: String, label: String): Passkey
    suspend fun delete(id: String): PasskeyRemoved
    suspend fun reauthMethods(): ReauthMethods
}

/** A refused passkey call: [code] is the server code; [readyAt] comes with `PASSKEY_PENDING`. */
class PasskeyApiException(
    status: io.ktor.http.HttpStatusCode,
    code: String?,
    val readyAt: String? = null,
) : ApiException(status, code ?: "Passkey request failed", code)

@Serializable
private data class PasskeyErrorBody(
    val error: String? = null,
    val message: String? = null,
    val readyAt: String? = null,
)

@Serializable
private data class LoginFinishBody(val ceremonyId: String, val credential: JsonObject, val username: String?)

@Serializable
private data class FinishBody(val ceremonyId: String, val credential: JsonObject)

@Serializable
private data class StepUpStartBody(val action: String, val target: String)

@Serializable
private data class RegisterStartBody(val label: String?)

@Serializable
private data class RenameBody(val label: String)

class PasskeyApiImpl @Inject constructor(
    private val httpClient: HttpClient,
) : PasskeyApi {

    private fun HttpRequestBuilder.android() {
        header(CLIENT_HEADER, "android")
    }

    private suspend inline fun <reified T> HttpResponse.ok(): T {
        if (!status.isSuccess()) {
            val body = runCatching { KaikuJson.decodeFromString<PasskeyErrorBody>(bodyAsText()) }.getOrNull()
            throw PasskeyApiException(status, normalizeErrorCode(body?.error), body?.readyAt)
        }
        return body()
    }

    override suspend fun loginStart(): CeremonyStart =
        httpClient.post("/auth/passkeys/login/start") { android(); setBody(JsonObject(emptyMap())) }.ok()

    override suspend fun loginFinish(ceremonyId: String, credential: JsonObject, username: String?): AuthResponse =
        httpClient.post("/auth/passkeys/login/finish") {
            android()
            setBody(LoginFinishBody(ceremonyId, credential, username?.trim()?.ifEmpty { null }))
        }.ok()

    override suspend fun stepUpStart(action: String, target: String): CeremonyStart =
        httpClient.post("/auth/passkeys/step-up/start") { android(); setBody(StepUpStartBody(action, target)) }.ok()

    override suspend fun stepUpFinish(ceremonyId: String, credential: JsonObject): StepUpGrant =
        httpClient.post("/auth/passkeys/step-up/finish") { android(); setBody(FinishBody(ceremonyId, credential)) }.ok()

    override suspend fun registerStart(label: String?): CeremonyStart =
        httpClient.post("/auth/passkeys/register/start") { android(); setBody(RegisterStartBody(label)) }.ok()

    override suspend fun registerFinish(ceremonyId: String, credential: JsonObject): Passkey =
        httpClient.post("/auth/passkeys/register/finish") { android(); setBody(FinishBody(ceremonyId, credential)) }.ok()

    override suspend fun list(): PasskeyList = httpClient.get("/auth/passkeys") { android() }.ok()

    override suspend fun rename(id: String, label: String): Passkey =
        httpClient.patch("/auth/passkeys/${id.encodeURLPathPart()}") {
            android()
            setBody(RenameBody(label))
        }.ok()

    override suspend fun delete(id: String): PasskeyRemoved =
        httpClient.delete("/auth/passkeys/${id.encodeURLPathPart()}") { android() }.ok()

    override suspend fun reauthMethods(): ReauthMethods = httpClient.get("/auth/reauth/methods") { android() }.ok()

    companion object {
        const val CLIENT_HEADER = "X-Kaiku-Client"
    }
}
