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

    /** Holds the next answer / deny until completed (a slow network). */
    var answerGate: kotlinx.coroutines.CompletableDeferred<Unit>? = null
    var denyGate: kotlinx.coroutines.CompletableDeferred<Unit>? = null

    override suspend fun answer(challengeId: String, decision: String, responseB64u: String, signatureB64u: String): AnswerResult {
        answerGate?.await()
        calls += "answer:$challengeId:$decision"
        lastAnswer = Triple(decision, responseB64u, signatureB64u)
        answerFailure?.let { throw it }
        return answerResult
    }

    override suspend fun deny(challengeId: String) { denyGate?.await(); calls += "deny:$challengeId" }
    override suspend fun release(challengeId: String) { calls += "release:$challengeId" }
    /** The origin the last device-login challenge was sent to (null: the configured server). */
    var deviceLoginOrigin: String? = null
    override suspend fun deviceLoginChallenge(userId: String, deviceId: String, origin: String?): ApprovalChallenge {
        deviceLoginOrigin = origin
        return challenge("deviceLoginChallenge:$userId:$deviceId")
    }

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

    /** The report hangs (until cancelled or timed out). */
    var reportHangs = false

    override suspend fun reportInvalidated(id: String): KeyDevice {
        calls += "reportInvalidated:$id"
        if (reportHangs) kotlinx.coroutines.awaitCancellation()
        return KeyDevice(id = id, label = "Pixel 8", state = KeyDeviceStatus.REVOKED)
    }
}

/**
 * Scripted C27 [io.wolftown.kaiku.data.api.LoginAttemptApi]. Every call is recorded with the
 * verifier it carried; a poll with nothing scripted hangs like a long-poll until cancelled.
 */
class FakeLoginAttemptApi : io.wolftown.kaiku.data.api.LoginAttemptApi {
    val calls = mutableListOf<String>()
    val challenges = mutableListOf<String>()
    val verifiers = mutableListOf<String>()
    var created: io.wolftown.kaiku.domain.model.LoginAttempt = attempt("a-qr", "qr", qr = "kaiku://login/v1?o=https%3A%2F%2Fchat.example.com&t=T1")
    var loginStep: io.wolftown.kaiku.domain.model.LoginStep? = null
    var loginFailure: Exception? = null
    var exchangeStep: io.wolftown.kaiku.domain.model.LoginStep? = null
    var exchangeFailure: Exception? = null
    var collectFailure: Exception? = null
    var totpFailure: Exception? = null
    var session: AuthResponse = AuthResponse(
        accessToken = "attempt-access", refreshToken = "attempt-refresh", expiresIn = 900, tokenType = "Bearer",
        sessionId = "55555555-5555-4555-8555-555555555555", assurance = "probation",
    )
    /** Answers for the next polls, in order. */
    val polls = ArrayDeque<suspend () -> io.wolftown.kaiku.domain.model.LoginAttempt>()

    override suspend fun createQr(clientChallenge: String): io.wolftown.kaiku.domain.model.LoginAttempt {
        calls += "createQr"; challenges += clientChallenge
        return created
    }

    override suspend fun login(username: String, password: String, clientChallenge: String): io.wolftown.kaiku.domain.model.LoginStep {
        calls += "login:$username"; challenges += clientChallenge
        loginFailure?.let { throw it }
        return loginStep ?: error("no login step scripted")
    }

    override suspend fun poll(id: String, verifier: String, waitSeconds: Int): io.wolftown.kaiku.domain.model.LoginAttempt {
        calls += "poll:$id"; verifiers += verifier
        val next = polls.removeFirstOrNull() ?: kotlinx.coroutines.awaitCancellation()
        return next()
    }

    override suspend fun collect(id: String, verifier: String): AuthResponse {
        calls += "collect:$id"; verifiers += verifier
        collectFailure?.let { throw it }
        return session
    }

    override suspend fun totp(id: String, verifier: String, code: String): AuthResponse {
        calls += "totp:$id:$code"; verifiers += verifier
        totpFailure?.let { throw it }
        return session
    }

    override suspend fun cancel(id: String, verifier: String) {
        calls += "cancel:$id"; verifiers += verifier
    }

    var oidcUrl: String = "https://idp.example.com/authorize?state=s1"
    var oidcStartFailure: Exception? = null

    override suspend fun oidcAuthorizeUrl(providerSlug: String, clientChallenge: String): String {
        calls += "oidcStart:$providerSlug"; challenges += clientChallenge
        oidcStartFailure?.let { throw it }
        return oidcUrl
    }

    override suspend fun exchangeOidc(code: String, verifier: String): io.wolftown.kaiku.domain.model.LoginStep {
        calls += "exchangeOidc:$code"; verifiers += verifier
        exchangeFailure?.let { throw it }
        return exchangeStep ?: error("no exchange step scripted")
    }

    companion object {
        fun attempt(
            id: String,
            kind: String,
            state: io.wolftown.kaiku.domain.model.AttemptState = io.wolftown.kaiku.domain.model.AttemptState.OPEN,
            qr: String? = null,
            totpAvailable: Boolean = false,
            expiresAt: String = java.time.Instant.now().plusSeconds(600).toString(),
        ) = io.wolftown.kaiku.domain.model.LoginAttempt(
            attemptId = id, kind = kind, state = state, expiresAt = expiresAt,
            qr = qr?.let { io.wolftown.kaiku.domain.model.AttemptQr(it, java.time.Instant.now().plusSeconds(60).toString()) },
            totpAvailable = totpAvailable,
        )
    }
}

/** S256 as C27 defines it: unpadded base64url SHA-256 of the verifier's ASCII bytes. */
fun s256(verifier: String): String = java.util.Base64.getUrlEncoder().withoutPadding().encodeToString(
    java.security.MessageDigest.getInstance("SHA-256").digest(verifier.toByteArray(Charsets.US_ASCII)),
)

/** A passkey step-up that records what it was asked for and grants it (LP's provider in tests). */
class RecordingPasskeyStepUp(
    private val answer: (String, String) -> Result<io.wolftown.kaiku.domain.model.StepUpGrant> =
        { action, target -> Result.success(io.wolftown.kaiku.domain.model.StepUpGrant(action, target, "passkey", "2026-11-03T12:05:00Z")) },
) : io.wolftown.kaiku.data.auth.StepUpProvider {
    val asked = mutableListOf<String>()
    override suspend fun stepUp(activity: android.app.Activity, action: String, target: String): Result<io.wolftown.kaiku.domain.model.StepUpGrant> {
        asked += "$action:$target"
        return answer(action, target)
    }
}

/**
 * The real composite step-up ([io.wolftown.kaiku.data.auth.KeyOrPasskeyStepUp] over
 * [io.wolftown.kaiku.data.auth.DeviceStepUpProvider]) on a phone that is not a key:
 * every grant comes from [passkeys].
 */
fun nonKeyPhoneStepUp(scope: kotlinx.coroutines.CoroutineScope, passkeys: io.wolftown.kaiku.data.auth.StepUpProvider): io.wolftown.kaiku.data.auth.KeyOrPasskeyStepUp {
    val authState = io.wolftown.kaiku.data.local.AuthState(scope).apply { setLoggedIn("11111111-1111-4111-8111-111111111111") }
    val repo = io.wolftown.kaiku.data.keydevice.KeyDeviceRepository(InMemoryKeyDeviceStore(), io.mockk.mockk(relaxed = true), authState, scope)
    val device = io.wolftown.kaiku.approval.DeviceStepUp(FakeApprovalApi(), io.mockk.mockk(), repo, { "device-1" }, io.mockk.mockk())
    return io.wolftown.kaiku.data.auth.KeyOrPasskeyStepUp(io.wolftown.kaiku.data.auth.DeviceStepUpProvider(device, fallback = passkeys))
}
