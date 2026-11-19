package io.wolftown.kaiku.data.auth

import android.app.Activity
import io.wolftown.kaiku.data.KaikuJson
import io.wolftown.kaiku.data.local.TokenStorage
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import java.net.IDN
import java.net.URI
import java.util.Locale
import javax.inject.Inject

/**
 * Which passkey site (WebAuthn RP ID) this app may use: the host of the
 * configured server, or a registrable parent domain of it. The app carries no
 * public-suffix list, so a parent must have at least two labels and end at a
 * label boundary; IP addresses and single-label hosts (local test servers)
 * match only exactly. Names compare in lower-case ASCII (punycode).
 */
object RpIdBinding {

    fun allowed(rpId: String?, serverUrl: String?): Boolean {
        val host = serverHost(serverUrl) ?: return false
        val rp = normalizedRpId(rpId) ?: return false
        if (rp == host) return true
        if (isIpLiteral(host) || isIpLiteral(rp)) return false
        if (!host.contains('.') || !rp.contains('.')) return false
        return host.endsWith(".$rp")
    }

    /** `rpId` of request options, or `rp.id` of creation options; null when absent or not a string. */
    fun rpIdOf(requestJson: String): String? {
        val obj = try {
            KaikuJson.parseToJsonElement(requestJson) as? JsonObject
        } catch (_: Exception) {
            null
        } ?: return null
        (obj["rpId"] as? JsonPrimitive)?.takeIf { it.isString }?.let { return it.content }
        val rp = obj["rp"] as? JsonObject ?: return null
        return (rp["id"] as? JsonPrimitive)?.takeIf { it.isString }?.content
    }

    private fun serverHost(serverUrl: String?): String? {
        if (serverUrl.isNullOrBlank()) return null
        val raw = try {
            URI(serverUrl.trim()).host
        } catch (_: Exception) {
            null
        } ?: return null
        val host = raw.lowercase(Locale.ROOT).removeSuffix(".")
        if (host.isEmpty()) return null
        if (host.startsWith("[")) return host.removePrefix("[").removeSuffix("]")
        return toAscii(host)
    }

    private fun normalizedRpId(rpId: String?): String? {
        if (rpId.isNullOrEmpty() || rpId != rpId.trim()) return null
        if (rpId.contains(':')) {
            // An IPv6 literal, with or without brackets; anything else with a colon (a port) is not an RP ID.
            val ip = rpId.lowercase(Locale.ROOT).removePrefix("[").removeSuffix("]")
            return ip.takeIf { IPV6.matches(it) }
        }
        // A trailing dot names another RP ID than the server's (its hash differs): refused.
        if (rpId.endsWith(".")) return null
        return toAscii(rpId.lowercase(Locale.ROOT))
    }

    private fun toAscii(name: String): String? {
        val ascii = try {
            IDN.toASCII(name, IDN.USE_STD3_ASCII_RULES)
        } catch (_: IllegalArgumentException) {
            return null
        }.lowercase(Locale.ROOT)
        if (ascii.isEmpty() || ascii.split('.').any { it.isEmpty() }) return null
        return ascii
    }

    private val IPV4 = Regex("^[0-9]{1,3}(\\.[0-9]{1,3}){3}$")
    private val IPV6 = Regex("^[0-9a-f:.]+$")

    private fun isIpLiteral(h: String): Boolean = IPV4.matches(h) || h.contains(':')
}

/**
 * The [PasskeyCredentials] the app uses: every Credential Manager call (sign-in,
 * step-up, adding a passkey) and every removal signal is checked against the
 * configured server first ([RpIdBinding]); a request for another site never
 * reaches Credential Manager and the ceremony stops with [PasskeyOutcome.WrongServer].
 */
class ServerBoundPasskeys internal constructor(
    private val inner: PasskeyCredentials,
    private val serverUrl: () -> String?,
) : PasskeyCredentials {

    @Inject
    constructor(inner: CredentialManagerPasskeys, tokenStorage: TokenStorage) : this(inner as PasskeyCredentials, tokenStorage::getServerUrl)

    private fun bound(requestJson: String): Boolean = RpIdBinding.allowed(RpIdBinding.rpIdOf(requestJson), serverUrl())

    override suspend fun get(activity: Activity, requestJson: String): PasskeyOutcome =
        if (bound(requestJson)) inner.get(activity, requestJson) else PasskeyOutcome.WrongServer

    override suspend fun create(activity: Activity, requestJson: String): PasskeyOutcome =
        // Creation options name the site as `rp.id`; a bare `rpId` there is not what Credential Manager reads.
        if (createBound(requestJson)) inner.create(activity, requestJson) else PasskeyOutcome.WrongServer

    private fun createBound(requestJson: String): Boolean {
        val obj = try {
            KaikuJson.parseToJsonElement(requestJson) as? JsonObject
        } catch (_: Exception) {
            null
        } ?: return false
        val rp = obj["rp"] as? JsonObject ?: return false
        val id = (rp["id"] as? JsonPrimitive)?.takeIf { it.isString }?.content
        return RpIdBinding.allowed(id, serverUrl())
    }

    override suspend fun signalRemoved(rpId: String, userHandle: String?, credentialId: String, remaining: List<String>?) {
        if (RpIdBinding.allowed(rpId, serverUrl())) inner.signalRemoved(rpId, userHandle, credentialId, remaining)
    }
}
