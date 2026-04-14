package io.wolftown.kaiku.ui.auth

import android.content.Context
import android.net.Uri
import android.util.Base64
import androidx.browser.customtabs.CustomTabsIntent
import io.wolftown.kaiku.data.local.TokenStorage
import io.wolftown.kaiku.data.repository.AuthRepository
import io.wolftown.kaiku.domain.model.User
import java.security.MessageDigest
import java.security.SecureRandom
import java.util.logging.Level
import java.util.logging.Logger
import javax.inject.Inject

/**
 * Manages the OIDC login flow using Chrome Custom Tabs.
 *
 * Flow:
 * 1. [launchOidcLogin] opens the server's OIDC authorize endpoint in a Custom Tab
 * 2. User authenticates with the external provider
 * 3. Provider redirects to server callback
 * 4. Server redirects to `kaiku://auth/callback` with tokens in query params
 * 5. Android intercepts the deep link (handled in [MainActivity])
 * 6. [handleCallback] extracts the tokens and completes login
 */
class OidcHandler @Inject constructor(
    private val tokenStorage: TokenStorage
) {

    companion object {
        const val REDIRECT_URI = "kaiku://auth/callback"
        private const val SCHEME = "kaiku"
        private const val HOST = "auth"
        private const val PATH = "/callback"
        private val logger = Logger.getLogger("OidcHandler")
    }

    /**
     * Opens a Chrome Custom Tab to the server's OIDC authorize endpoint.
     *
     * @param context Activity context needed to launch the Custom Tab
     * @param providerSlug The OIDC provider slug (e.g., "google", "github")
     */
    fun launchOidcLogin(context: Context, providerSlug: String) {
        val serverUrl = tokenStorage.getServerUrl()
            ?: throw IllegalStateException("Server URL not configured")

        // Generate PKCE code_verifier (43-128 chars, URL-safe base64)
        val codeVerifier = ByteArray(32).also { SecureRandom().nextBytes(it) }
            .let { Base64.encodeToString(it, Base64.URL_SAFE or Base64.NO_WRAP or Base64.NO_PADDING) }

        // Compute code_challenge = BASE64URL(SHA256(code_verifier))
        val codeChallenge = MessageDigest.getInstance("SHA-256")
            .digest(codeVerifier.toByteArray(Charsets.US_ASCII))
            .let { Base64.encodeToString(it, Base64.URL_SAFE or Base64.NO_WRAP or Base64.NO_PADDING) }

        // Generate state nonce for CSRF protection
        val state = ByteArray(32).also { SecureRandom().nextBytes(it) }
            .let { Base64.encodeToString(it, Base64.URL_SAFE or Base64.NO_WRAP or Base64.NO_PADDING) }

        // Persist to EncryptedSharedPreferences for validation in callback
        tokenStorage.saveOidcPkceState(codeVerifier, state)

        val authUrl = "$serverUrl/auth/oidc/authorize/$providerSlug" +
            "?redirect_uri=${Uri.encode(REDIRECT_URI)}" +
            "&code_challenge=$codeChallenge" +
            "&code_challenge_method=S256" +
            "&state=$state"
        try {
            val customTabIntent = CustomTabsIntent.Builder().build()
            customTabIntent.launchUrl(context, Uri.parse(authUrl))
        } catch (e: Exception) {
            logger.log(Level.WARNING, "Failed to launch OIDC login", e)
            throw e
        }
    }

    /**
     * Checks whether the given [Uri] is an OIDC callback deep link.
     */
    fun isOidcCallback(uri: Uri): Boolean {
        return uri.scheme == SCHEME && uri.host == HOST && uri.path == PATH
    }

    /**
     * Handles the OIDC callback deep link by extracting tokens and completing login.
     *
     * The server redirects to `kaiku://auth/callback` with the following query params:
     * - `access_token` — JWT access token
     * - `refresh_token` — refresh token
     * - `expires_in` — token expiry in seconds
     * - `setup_required` — whether server setup is still needed (optional)
     *
     * If the redirect contains `code` and `state` instead (authorization code flow),
     * falls back to exchanging the code via [AuthRepository.exchangeOidcCode].
     *
     * @param uri The deep link URI
     * @param authRepository Repository to complete the login
     * @return [Result] containing the authenticated [User] or a failure
     */
    suspend fun handleCallback(uri: Uri, authRepository: AuthRepository): Result<User> {
        // Validate state nonce against saved value (CSRF protection)
        val savedState = tokenStorage.getOidcState()
        val uriState = uri.getQueryParameter("state")
        if (savedState == null || uriState != savedState) {
            tokenStorage.clearOidcPkceState()
            return Result.failure(IllegalStateException("OIDC state mismatch — possible CSRF"))
        }

        // Primary path: server already exchanged the code and redirected with tokens
        val accessToken = uri.getQueryParameter("access_token")
        val refreshToken = uri.getQueryParameter("refresh_token")
        val expiresInStr = uri.getQueryParameter("expires_in")

        if (accessToken != null && expiresInStr != null) {
            tokenStorage.clearOidcPkceState()
            val expiresIn = expiresInStr.toIntOrNull()
                ?: return Result.failure(IllegalArgumentException("Invalid expires_in value"))
            return authRepository.completeOidcLogin(
                accessToken = accessToken,
                refreshToken = refreshToken,
                expiresIn = expiresIn
            )
        }

        // Fallback path: redirect contains authorization code (code + state)
        val code = uri.getQueryParameter("code")
            ?: return Result.failure(
                IllegalArgumentException("OIDC callback missing both tokens and authorization code")
            )

        val codeVerifier = tokenStorage.getOidcCodeVerifier()
        tokenStorage.clearOidcPkceState()
        return authRepository.exchangeOidcCode(code, uriState, codeVerifier)
    }
}
