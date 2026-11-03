package io.wolftown.kaiku.data.auth

import android.app.Activity
import io.wolftown.kaiku.data.KaikuJson
import io.wolftown.kaiku.data.api.PasskeyApi
import io.wolftown.kaiku.data.api.PasskeyApiException
import io.wolftown.kaiku.data.local.TokenStorage
import io.wolftown.kaiku.data.repository.AuthRepository
import io.wolftown.kaiku.domain.model.CeremonyStart
import io.wolftown.kaiku.domain.model.StepUpGrant
import io.wolftown.kaiku.domain.model.User
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import javax.inject.Inject
import kotlin.coroutines.cancellation.CancellationException

/** Why a passkey call gave no result; [code] is a server or ceremony code. */
sealed class PasskeyFailure(val code: String) : Exception(code) {
    data object Cancelled : PasskeyFailure("PASSKEY_CANCELLED")
    data object NoCredential : PasskeyFailure("PASSKEY_NO_CREDENTIAL")
    data object NotAssociated : PasskeyFailure("PASSKEY_NOT_ASSOCIATED")
    /** `PASSKEY_UNKNOWN`: the credential was signalled; "removed from Kaiku". */
    data object Removed : PasskeyFailure("PASSKEY_UNKNOWN")
    /** `PASSKEY_PENDING`: never signalled unknown. */
    data class Pending(val readyAt: String?) : PasskeyFailure("PASSKEY_PENDING")
    class Server(code: String) : PasskeyFailure(code)
}

/** The `publicKey` object of the server's options, as Credential Manager wants it. */
internal fun publicKeyJson(start: CeremonyStart): String =
    (start.options["publicKey"] ?: start.options).toString()

private fun rpIdOf(start: CeremonyStart): String? =
    (start.options["publicKey"] as? JsonObject)?.get("rpId")?.jsonPrimitive?.contentOrNull

/** Runs a Credential Manager call; anything but a credential is a [PasskeyFailure]. */
internal suspend fun ceremony(run: suspend () -> PasskeyOutcome): JsonObject =
    when (val outcome = run()) {
        is PasskeyOutcome.Success -> KaikuJson.parseToJsonElement(outcome.responseJson).jsonObject
        PasskeyOutcome.Cancelled -> throw PasskeyFailure.Cancelled
        PasskeyOutcome.NoCredential -> throw PasskeyFailure.NoCredential
        PasskeyOutcome.NotAssociated -> throw PasskeyFailure.NotAssociated
        is PasskeyOutcome.Failed -> throw PasskeyFailure.Server(outcome.code)
    }

/** Maps a server refusal; `PASSKEY_UNKNOWN` signals the credential first. */
internal suspend fun serverFailure(
    e: PasskeyApiException,
    credentials: PasskeyCredentials,
    start: CeremonyStart,
    credential: JsonObject,
): PasskeyFailure = when (e.normalizedCode) {
    "PASSKEY_UNKNOWN" -> {
        val id = credential["id"]?.jsonPrimitive?.contentOrNull
        val rpId = rpIdOf(start)
        if (id != null && rpId != null) credentials.signalRemoved(rpId, null, id, null)
        PasskeyFailure.Removed
    }
    "PASSKEY_PENDING" -> PasskeyFailure.Pending(e.readyAt)
    else -> PasskeyFailure.Server(e.normalizedCode ?: "PASSKEY_FAILED")
}

/** Passkey sign-in in the app (F1: Credential Manager → a full session). */
class PasskeyAuth @Inject constructor(
    private val api: PasskeyApi,
    private val credentials: PasskeyCredentials,
    private val authRepository: AuthRepository,
    private val tokenStorage: TokenStorage,
) {
    suspend fun loginWithPasskey(activity: Activity, username: String?): Result<User> = try {
        // Nothing is started (or minted on the server) while a session runs
        // or without a server; checked again before the finish.
        val serverUrl = tokenStorage.getServerUrl()
        if (serverUrl == null || !authRepository.canStartSignIn()) throw PasskeyFailure.Server("PASSKEY_FAILED")
        val start = api.loginStart()
        val credential = ceremony { credentials.get(activity, publicKeyJson(start)) }
        if (!authRepository.canStartSignIn() || tokenStorage.getServerUrl() != serverUrl) {
            throw PasskeyFailure.Cancelled
        }
        val response = try {
            api.loginFinish(start.ceremonyId, credential, username)
        } catch (e: PasskeyApiException) {
            throw serverFailure(e, credentials, start, credential)
        }
        authRepository.installNativeSignIn(response, serverUrl)
    } catch (e: CancellationException) {
        throw e
    } catch (e: Exception) {
        Result.failure(e)
    }
}

/** LP's [StepUpProvider]: a passkey assertion for one action and target. */
class PasskeyStepUp @Inject constructor(
    private val api: PasskeyApi,
    private val credentials: PasskeyCredentials,
) : StepUpProvider {
    override suspend fun stepUp(activity: Activity, action: String, target: String): Result<StepUpGrant> = try {
        val start = api.stepUpStart(action, target)
        val credential = ceremony { credentials.get(activity, publicKeyJson(start)) }
        try {
            Result.success(api.stepUpFinish(start.ceremonyId, credential))
        } catch (e: PasskeyApiException) {
            throw serverFailure(e, credentials, start, credential)
        }
    } catch (e: CancellationException) {
        throw e
    } catch (e: Exception) {
        Result.failure(e)
    }
}
