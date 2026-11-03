package io.wolftown.kaiku.data.auth

import android.app.Activity
import android.content.Context
import androidx.credentials.CreateCredentialResponse
import androidx.credentials.CreatePublicKeyCredentialRequest
import androidx.credentials.CreatePublicKeyCredentialResponse
import androidx.credentials.CredentialManager
import androidx.credentials.GetCredentialRequest
import androidx.credentials.GetPublicKeyCredentialOption
import androidx.credentials.PublicKeyCredential
import androidx.credentials.SignalAllAcceptedCredentialIdsRequest
import androidx.credentials.SignalUnknownCredentialRequest
import androidx.credentials.exceptions.CreateCredentialCancellationException
import androidx.credentials.exceptions.CreateCredentialException
import androidx.credentials.exceptions.GetCredentialCancellationException
import androidx.credentials.exceptions.GetCredentialException
import androidx.credentials.exceptions.NoCredentialException
import androidx.credentials.exceptions.domerrors.DomError
import androidx.credentials.exceptions.domerrors.InvalidStateError
import androidx.credentials.exceptions.domerrors.NotAllowedError
import androidx.credentials.exceptions.domerrors.SecurityError
import androidx.credentials.exceptions.publickeycredential.CreatePublicKeyCredentialDomException
import androidx.credentials.exceptions.publickeycredential.GetPublicKeyCredentialDomException
import dagger.hilt.android.qualifiers.ApplicationContext
import java.util.logging.Level
import java.util.logging.Logger
import javax.inject.Inject
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put

/** The result of a Credential Manager passkey call. */
sealed interface PasskeyOutcome {
    /** The `PublicKeyCredential` JSON, passed to the server unchanged. */
    data class Success(val responseJson: String) : PasskeyOutcome
    data object Cancelled : PasskeyOutcome
    data object NoCredential : PasskeyOutcome
    /** The RP domain is not verified for this app (`assetlinks.json`). */
    data object NotAssociated : PasskeyOutcome
    data class Failed(val code: String) : PasskeyOutcome
}

/** Credential Manager for passkeys (C29; no Custom Tab fallback in v1.0). */
interface PasskeyCredentials {
    /** `requestJson` = the `publicKey` object of the server's options. */
    suspend fun get(activity: Activity, requestJson: String): PasskeyOutcome
    suspend fun create(activity: Activity, requestJson: String): PasskeyOutcome
    /** After a removal or `PASSKEY_UNKNOWN`; best effort, never throws. */
    suspend fun signalRemoved(rpId: String, userHandle: String?, credentialId: String, remaining: List<String>?)
}

class CredentialManagerPasskeys @Inject constructor(
    @ApplicationContext private val context: Context,
) : PasskeyCredentials {
    private val manager by lazy { CredentialManager.create(context) }

    override suspend fun get(activity: Activity, requestJson: String): PasskeyOutcome = try {
        val response = manager.getCredential(
            activity,
            GetCredentialRequest(listOf(GetPublicKeyCredentialOption(requestJson))),
        )
        when (val credential = response.credential) {
            is PublicKeyCredential -> PasskeyOutcome.Success(credential.authenticationResponseJson)
            else -> PasskeyOutcome.Failed("PASSKEY_FAILED")
        }
    } catch (_: GetCredentialCancellationException) {
        PasskeyOutcome.Cancelled
    } catch (_: NoCredentialException) {
        PasskeyOutcome.NoCredential
    } catch (e: GetPublicKeyCredentialDomException) {
        mapDomError(e.domError)
    } catch (_: GetCredentialException) {
        PasskeyOutcome.Failed("PASSKEY_FAILED")
    }

    override suspend fun create(activity: Activity, requestJson: String): PasskeyOutcome = try {
        val response: CreateCredentialResponse =
            manager.createCredential(activity, CreatePublicKeyCredentialRequest(requestJson))
        when (response) {
            is CreatePublicKeyCredentialResponse -> PasskeyOutcome.Success(response.registrationResponseJson)
            else -> PasskeyOutcome.Failed("PASSKEY_FAILED")
        }
    } catch (_: CreateCredentialCancellationException) {
        PasskeyOutcome.Cancelled
    } catch (e: CreatePublicKeyCredentialDomException) {
        mapDomError(e.domError)
    } catch (_: CreateCredentialException) {
        PasskeyOutcome.Failed("PASSKEY_FAILED")
    }

    override suspend fun signalRemoved(
        rpId: String,
        userHandle: String?,
        credentialId: String,
        remaining: List<String>?,
    ) {
        // Best effort: a provider without signal support keeps offering the
        // credential until the server refuses it.
        runCatching {
            val unknown = buildJsonObject {
                put("rpId", rpId)
                put("credentialId", credentialId)
            }
            manager.signalCredentialState(SignalUnknownCredentialRequest(unknown.toString()))
        }.onFailure { logger.log(Level.FINE, "signalUnknownCredential not delivered", it) }
        if (userHandle != null && remaining != null) {
            runCatching {
                val accepted = buildJsonObject {
                    put("rpId", rpId)
                    put("userId", userHandle)
                    put("allAcceptedCredentialIds", JsonArray(remaining.map(::JsonPrimitive)))
                }
                manager.signalCredentialState(SignalAllAcceptedCredentialIdsRequest(accepted.toString()))
            }.onFailure { logger.log(Level.FINE, "signalAllAcceptedCredentials not delivered", it) }
        }
    }

    private companion object {
        val logger: Logger = Logger.getLogger("CredentialManagerPasskeys")
    }
}

/**
 * A WebAuthn DOM error by its type (never by provider message text):
 * `SecurityError` = the domain is not verified for this app
 * (`assetlinks.json`); `InvalidStateError` = an excluded credential, i.e.
 * already registered; `NotAllowedError` = the user dismissed the sheet.
 */
internal fun mapDomError(error: DomError): PasskeyOutcome = when (error) {
    is SecurityError -> PasskeyOutcome.NotAssociated
    is InvalidStateError -> PasskeyOutcome.Failed("PASSKEY_ALREADY_REGISTERED")
    is NotAllowedError -> PasskeyOutcome.Cancelled
    else -> PasskeyOutcome.Failed("PASSKEY_FAILED")
}
