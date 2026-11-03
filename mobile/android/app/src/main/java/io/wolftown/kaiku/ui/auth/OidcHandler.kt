package io.wolftown.kaiku.ui.auth

import android.content.Context
import android.net.Uri
import androidx.browser.customtabs.CustomTabsIntent
import io.wolftown.kaiku.data.local.TokenStorage
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import java.net.URLEncoder
import java.util.logging.Level
import java.util.logging.Logger
import javax.inject.Inject

/** What an OIDC callback may carry (C27, C29 delivery): only a one-time code. */
sealed interface OidcCallback {
    data class Code(val code: String) : OidcCallback {
        override fun toString(): String = "OidcCallback.Code"
    }
}

/**
 * The OIDC sign-in through a Chrome Custom Tab (F3 OIDC variant, C27):
 *
 * 1. [launchOidcLogin] opens `{server}/auth/oidc/authorize/{provider}` with the
 *    attempt's `client_challenge` (S256 of a verifier that only the sign-in
 *    screen holds, in memory);
 * 2. the provider and the server redirect to `kaiku://auth/callback?code=…`;
 * 3. [handleCallback] reads only that one-time code, which the screen redeems
 *    at `POST /auth/oidc/desktop/exchange {code, verifier}` for a `LoginStep`.
 *
 * A callback carrying tokens is refused whole: a link can never sign the app in.
 * Nothing about the flow is stored on disk.
 */
class OidcHandler @Inject constructor(
    private val tokenStorage: TokenStorage
) {

    companion object {
        const val REDIRECT_URI = "kaiku://auth/callback"
        private const val SCHEME = "kaiku"
        private const val HOST = "auth"
        private const val PATH = "/callback"
        /** Parameters of the old token redirect; their presence voids the callback. */
        private val TOKEN_PARAMS = setOf("access_token", "refresh_token", "expires_in", "id_token", "token")
        private val logger = Logger.getLogger("OidcHandler")
    }

    /** The authorize URL on [serverUrl] for [providerSlug], bound to [clientChallenge]. */
    fun authorizeUrl(serverUrl: String, providerSlug: String, clientChallenge: String): String =
        serverUrl.trimEnd('/') + "/auth/oidc/authorize/" + enc(providerSlug) +
            "?redirect_uri=" + enc(REDIRECT_URI) +
            "&client_challenge=" + enc(clientChallenge)

    /**
     * Opens a Chrome Custom Tab to the configured server's OIDC authorize endpoint.
     *
     * @param context Activity context needed to launch the Custom Tab
     * @param providerSlug The OIDC provider slug (e.g., "google", "github")
     * @param clientChallenge S256 of the sign-in attempt's verifier
     */
    fun launchOidcLogin(context: Context, providerSlug: String, clientChallenge: String) {
        val serverUrl = tokenStorage.getServerUrl()
            ?: throw IllegalStateException("Server URL not configured")
        // A verifier an older version wrote to disk is never used again.
        tokenStorage.clearOidcPkceState()
        try {
            CustomTabsIntent.Builder().build()
                .launchUrl(context, Uri.parse(authorizeUrl(serverUrl, providerSlug, clientChallenge)))
        } catch (e: Exception) {
            logger.log(Level.WARNING, "Failed to launch OIDC login", e)
            throw e
        }
    }

    /** Whether [uri] is an OIDC callback deep link (`kaiku://auth/callback`). */
    fun isOidcCallback(uri: Uri): Boolean {
        return uri.scheme == SCHEME && uri.host == HOST && uri.path == PATH
    }

    /**
     * The one-time code of an OIDC callback, or null: not a callback, no code, a
     * provider error, or any token parameter (the pre-v1.0 token redirect),
     * which installs nothing.
     */
    fun handleCallback(uri: Uri): OidcCallback? {
        if (!isOidcCallback(uri)) return null
        val names = uri.queryParameterNames
        if (names.any { it in TOKEN_PARAMS }) {
            logger.warning("OIDC callback with tokens refused")
            return null
        }
        if (uri.getQueryParameter("error") != null) return null
        val code = uri.getQueryParameter("code")?.takeIf { it.isNotBlank() } ?: return null
        return OidcCallback.Code(code)
    }

    private fun enc(s: String): String = URLEncoder.encode(s, Charsets.UTF_8.name())
}

/**
 * The latest OIDC callback from a deep link, in memory only, until the sign-in
 * screen that started the flow (and holds its verifier) takes it.
 */
object PendingOidcCallback {
    private val _pending = MutableStateFlow<Uri?>(null)
    val pending: StateFlow<Uri?> = _pending.asStateFlow()

    fun post(uri: Uri) {
        _pending.value = uri
    }

    fun take(): Uri? {
        val uri = _pending.value
        _pending.value = null
        return uri
    }
}
