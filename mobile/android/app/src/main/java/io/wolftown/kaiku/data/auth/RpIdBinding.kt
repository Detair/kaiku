package io.wolftown.kaiku.data.auth

import android.app.Activity
import android.content.Context
import dagger.hilt.android.qualifiers.ApplicationContext
import io.wolftown.kaiku.data.KaikuJson
import io.wolftown.kaiku.data.local.TokenStorage
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import okhttp3.HttpUrl.Companion.toHttpUrlOrNull
import javax.inject.Inject

/**
 * Which passkey site (WebAuthn RP ID) this app may use: exactly the host of the configured
 * server. A Kaiku server always names its own public host as the RP ID, never a parent domain,
 * so nothing else is ever needed (and no public-suffix question arises).
 *
 * - The server URL is read with OkHttp's `HttpUrl`, the parser the app connects with: `https`
 *   only (the app never talks cleartext), no user name or password in it. Its host is lower-case
 *   ASCII, an internationalised name in punycode, an IPv6 address without brackets; the port and
 *   path do not matter.
 * - The RP ID must already be in that canonical form: lower-case ASCII letters, digits, `-` and
 *   `.`, no empty label, no leading or trailing dot, or an IP address equal to the host. Nothing
 *   is converted: upper case, any non-ASCII or invisible character, white space or a port refuse
 *   it. A server with an internationalised name sends its RP ID in punycode (the server takes it
 *   from its parsed public URL, whose host is already ASCII).
 * - Side effect: an app whose server address is not the server's public host (another name for
 *   the same server) gets the "different website" message for passkeys.
 */
object RpIdBinding {

    fun allowed(rpId: String?, serverUrl: String?): Boolean {
        val host = serverHost(serverUrl) ?: return false
        if (rpId == null || !isCanonical(rpId)) return false
        return rpId == host
    }

    private fun serverHost(serverUrl: String?): String? {
        val url = serverUrl?.trim()?.toHttpUrlOrNull() ?: return null
        if (!url.isHttps || url.username.isNotEmpty() || url.password.isNotEmpty()) return null
        // A fully qualified name's trailing dot names the same server.
        return url.host.removeSuffix(".").ifEmpty { null }
    }

    private val LABEL = Regex("^[a-z0-9-]+$")
    private val IPV6 = Regex("^[0-9a-f:.]+$")

    private fun isCanonical(rp: String): Boolean {
        if (rp.isEmpty() || rp.length > 253) return false
        if (rp.contains(':')) return IPV6.matches(rp)
        return rp.split('.').all { it.isNotEmpty() && LABEL.matches(it) }
    }
}

/**
 * The [PasskeyCredentials] the app uses: every Credential Manager call (sign-in,
 * step-up, adding a passkey) and every removal signal is checked against the
 * configured server first ([RpIdBinding]); a request for another site never
 * reaches Credential Manager and the ceremony stops with [PasskeyOutcome.WrongServer].
 *
 * Sign-in and step-up options name the site only in a string `rpId`, creation options only in a
 * string `rp.id` (what Credential Manager reads for each). What is passed on is the parsed request
 * written out again, so the RP ID that was checked is the only one in it (a repeated key in the
 * server's text cannot carry a second, unchecked one).
 */
class ServerBoundPasskeys internal constructor(
    private val inner: PasskeyCredentials,
    private val serverUrl: () -> String?,
) : PasskeyCredentials {

    /** The only way the app gets to Credential Manager: [CredentialManagerPasskeys] is not injectable. */
    @Inject
    constructor(@ApplicationContext context: Context, tokenStorage: TokenStorage) :
        this(CredentialManagerPasskeys(context), tokenStorage::getServerUrl)

    override suspend fun get(activity: Activity, requestJson: String): PasskeyOutcome {
        val checked = checked(requestJson) { it["rpId"] } ?: return PasskeyOutcome.WrongServer
        return inner.get(activity, checked)
    }

    override suspend fun create(activity: Activity, requestJson: String): PasskeyOutcome {
        val checked = checked(requestJson) { (it["rp"] as? JsonObject)?.get("id") } ?: return PasskeyOutcome.WrongServer
        return inner.create(activity, checked)
    }

    /** The request written out from its parsed form, or null when its RP ID is not the server's host. */
    private fun checked(requestJson: String, rpIdField: (JsonObject) -> JsonElement?): String? {
        val obj = try {
            KaikuJson.parseToJsonElement(requestJson) as? JsonObject
        } catch (_: Exception) {
            null
        } ?: return null
        val rpId = (rpIdField(obj) as? JsonPrimitive)?.takeIf { it.isString }?.content ?: return null
        if (!RpIdBinding.allowed(rpId, serverUrl())) return null
        return obj.toString()
    }

    override suspend fun signalRemoved(rpId: String, userHandle: String?, credentialId: String, remaining: List<String>?) {
        if (RpIdBinding.allowed(rpId, serverUrl())) inner.signalRemoved(rpId, userHandle, credentialId, remaining)
    }
}
