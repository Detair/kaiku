package io.wolftown.kaiku.domain

import java.net.URI
import java.net.URLDecoder

/** What a scanned code (or an opened link) is, before anything is sent. */
sealed interface ScanPayload {
    data class Login(val origin: String, val tokenB64u: String) : ScanPayload {
        override fun toString() = "Login(origin=$origin)"
    }
    data class Setup(val origin: String, val tokenB64u: String) : ScanPayload {
        override fun toString() = "Setup(origin=$origin)"
    }
    /** A Kaiku code of another server than the one this app is tied to. */
    data class OtherServer(val origin: String) : ScanPayload
    /** A newer payload version: "Update Kaiku". */
    data object NeedsUpdate : ScanPayload
    /** `kaiku://qr/login?…` from an older desktop app. */
    data object OldKaikuCode : ScanPayload
    data object NotKaiku : ScanPayload
}

/** The scanner in the full app accepts both; the setup entry only setup codes. */
enum class ScanMode { SIGN_IN_OR_SETUP, SETUP_ONLY }

/**
 * C27 payloads. Login: `kaiku://login/v1?o=<percent-encoded origin>&t=<43-char b64u>`
 * (scanner only, never a link). Setup: `{PUBLIC_URL}/setup#v=1&t=<43-char b64u>`
 * (the token is in the fragment) and its app form `kaiku://setup/v1?o=&t=`.
 * Parsing checks the shape only; [pin] then refuses another server's code
 * before any network call (B.1 origin pinning).
 */
object KaikuPayloads {
    private val TOKEN = Regex("^[A-Za-z0-9_-]{43}$")
    private val VERSION_PATH = Regex("^/v(\\d+)$")
    private val DEV_HOSTS = setOf("localhost", "127.0.0.1", "[::1]")

    fun parse(raw: String): ScanPayload {
        val text = raw.trim()
        val uri = try {
            URI(text)
        } catch (_: Exception) {
            return ScanPayload.NotKaiku
        }
        val scheme = uri.scheme?.lowercase() ?: return ScanPayload.NotKaiku
        return when (scheme) {
            "kaiku" -> parseKaiku(uri)
            "https", "http" -> parseSetupUrl(uri, scheme)
            else -> ScanPayload.NotKaiku
        }
    }

    private fun parseKaiku(uri: URI): ScanPayload {
        val host = uri.host?.lowercase() ?: uri.rawAuthority?.lowercase()
        val path = uri.rawPath ?: ""
        if (host == "qr" && path == "/login") return ScanPayload.OldKaikuCode
        if (host != "login" && host != "setup") return ScanPayload.NotKaiku
        val version = VERSION_PATH.matchEntire(path)?.groupValues?.get(1) ?: return ScanPayload.NotKaiku
        if (version != "1") return ScanPayload.NeedsUpdate
        val q = params(uri.rawQuery)
        val origin = q["o"]?.let(::canonicalOrigin) ?: return ScanPayload.NotKaiku
        if (!secureOrDev(origin)) return ScanPayload.NotKaiku
        val token = q["t"]?.takeIf { TOKEN.matches(it) } ?: return ScanPayload.NotKaiku
        return if (host == "login") ScanPayload.Login(origin, token) else ScanPayload.Setup(origin, token)
    }

    private fun parseSetupUrl(uri: URI, scheme: String): ScanPayload {
        val path = uri.rawPath ?: return ScanPayload.NotKaiku
        if (path != "/setup" && !path.endsWith("/setup")) return ScanPayload.NotKaiku
        val origin = canonicalOrigin(uri.toString()) ?: return ScanPayload.NotKaiku
        if (scheme == "http" && !secureOrDev(origin)) return ScanPayload.NotKaiku
        val f = params(uri.rawFragment)
        val v = f["v"] ?: return ScanPayload.NotKaiku
        if (v != "1") return ScanPayload.NeedsUpdate
        val token = f["t"]?.takeIf { TOKEN.matches(it) } ?: return ScanPayload.NotKaiku
        return ScanPayload.Setup(origin, token)
    }

    private fun secureOrDev(origin: String): Boolean =
        origin.startsWith("https://") || DEV_HOSTS.any { origin == "http://$it" || origin.startsWith("http://$it:") }

    private fun params(raw: String?): Map<String, String> =
        raw.orEmpty().split('&').filter { it.isNotEmpty() }.mapNotNull { part ->
            val i = part.indexOf('=')
            if (i <= 0) null else part.substring(0, i) to decode(part.substring(i + 1))
        }.toMap()

    private fun decode(s: String): String =
        // '+' is a literal in b64u tokens' neighbours only by mistake; keep it so the token check fails.
        URLDecoder.decode(s.replace("+", "%2B"), "UTF-8")

    /**
     * `scheme://host[:port]` with scheme and host in lower case, the default
     * port dropped and no path; null for anything but http(s) with a host.
     */
    fun canonicalOrigin(url: String): String? {
        val uri = try {
            URI(url.trim())
        } catch (_: Exception) {
            return null
        }
        val scheme = uri.scheme?.lowercase() ?: return null
        if (scheme != "https" && scheme != "http") return null
        val host = uri.host?.lowercase()?.removeSuffix(".")?.takeIf { it.isNotEmpty() } ?: return null
        if (uri.rawUserInfo != null) return null
        val port = uri.port
        val defaultPort = if (scheme == "https") 443 else 80
        return if (port == -1 || port == defaultPort) "$scheme://$host" else "$scheme://$host:$port"
    }

    /** Another server's code becomes [ScanPayload.OtherServer] when the app is tied to [pinnedOrigin]. */
    fun pin(p: ScanPayload, pinnedOrigin: String?): ScanPayload {
        val pinned = pinnedOrigin?.let(::canonicalOrigin) ?: return p
        return when (p) {
            is ScanPayload.Login -> if (p.origin == pinned) p else ScanPayload.OtherServer(p.origin)
            is ScanPayload.Setup -> if (p.origin == pinned) p else ScanPayload.OtherServer(p.origin)
            else -> p
        }
    }
}
