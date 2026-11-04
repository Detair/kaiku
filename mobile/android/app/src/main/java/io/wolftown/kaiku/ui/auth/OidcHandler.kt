package io.wolftown.kaiku.ui.auth

import android.content.Context
import android.net.Uri
import androidx.browser.customtabs.CustomTabsIntent
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.getAndUpdate
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
 * 1. the sign-in screen fetches the native start
 *    `GET {server}/auth/oidc/authorize/{provider}?client_challenge=…` with
 *    `X-Kaiku-Client: android` ([io.wolftown.kaiku.data.api.LoginAttemptApi.oidcAuthorizeUrl];
 *    a Custom Tab cannot send that header, and without it the server answers
 *    400 `OIDC_CLIENT_OUTDATED`); the answer is JSON `{url}`, the provider page;
 * 2. [openProvider] opens that page; the provider and the server redirect to
 *    `kaiku://auth/callback?code=…`;
 * 3. [handleCallback] reads only that one-time code, which the screen redeems
 *    at `POST /auth/oidc/desktop/exchange {code, verifier}` for a `LoginStep`.
 *
 * The `client_challenge` is S256 of a verifier that only the sign-in screen
 * holds, in memory. A callback carrying tokens is refused whole: a link can
 * never sign the app in. Nothing about the flow is stored on disk.
 */
class OidcHandler @Inject constructor() {

    companion object {
        private const val SCHEME = "kaiku"
        private const val HOST = "auth"
        private const val PATH = "/callback"
        /** Parameters of the old token redirect; their presence voids the callback. */
        private val TOKEN_PARAMS = setOf("access_token", "refresh_token", "expires_in", "id_token", "token")
        private val logger = Logger.getLogger("OidcHandler")
    }

    /**
     * Opens the provider page of a native start (its `url`) in a Chrome Custom Tab.
     *
     * @param context Activity context needed to launch the Custom Tab
     * @param providerUrl The `url` of the start's answer (an `https` page)
     */
    fun openProvider(context: Context, providerUrl: String) {
        try {
            CustomTabsIntent.Builder().build().launchUrl(context, Uri.parse(providerUrl))
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

    /** At most once, also with two takers at the same time. */
    fun take(): Uri? = _pending.getAndUpdate { null }
}
