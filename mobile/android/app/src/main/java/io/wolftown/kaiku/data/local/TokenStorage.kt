package io.wolftown.kaiku.data.local

import android.content.SharedPreferences
import java.util.logging.Logger
import javax.inject.Inject

/**
 * Persists the session tokens.
 *
 * All token writers ([saveTokens], [saveTokensIfRefreshTokenIs], [clear],
 * [clearIfRefreshTokenIs], [saveServerUrl]) are synchronized on this instance,
 * so the conditional variants used by the token refresh are atomic with respect
 * to login and logout.
 *
 * [getSessionGeneration] changes whenever a different session may start (login,
 * logout, server change) but not when a refresh rotates the current session's
 * tokens. Callers capture it to detect that the session they started from is gone.
 */
class TokenStorage @Inject constructor(
    private val prefs: SharedPreferences
) {

    companion object {
        private const val KEY_ACCESS_TOKEN = "access_token"
        private const val KEY_REFRESH_TOKEN = "refresh_token"
        private const val KEY_EXPIRES_AT = "expires_at"
        private const val KEY_USER_ID = "user_id"
        private const val KEY_SERVER_URL = "server_url"
        private const val KEY_OIDC_CODE_VERIFIER = "oidc_code_verifier"
        private const val KEY_OIDC_STATE = "oidc_state"
        private val logger = Logger.getLogger("TokenStorage")
    }

    @Volatile
    private var sessionGeneration = 0L

    /** Changes on login, logout and server change; unchanged by a refresh rotation. */
    fun getSessionGeneration(): Long = sessionGeneration

    /** Stores a new session (login). Starts a new session generation. */
    @Synchronized
    fun saveTokens(
        accessToken: String,
        refreshToken: String?,
        expiresIn: Int,
        userId: String
    ) {
        // Bump before writing, so a reader that sees the new tokens also sees the new generation.
        sessionGeneration++
        writeTokens(accessToken, refreshToken, expiresIn, userId)
    }

    /**
     * Stores rotated tokens only if the stored refresh token is still
     * [expectedRefreshToken], i.e. the session was neither logged out nor
     * replaced while the refresh was in flight. Keeps the session generation.
     *
     * @return true if the tokens were written, false if nothing was written.
     */
    @Synchronized
    fun saveTokensIfRefreshTokenIs(
        expectedRefreshToken: String,
        accessToken: String,
        refreshToken: String?,
        expiresIn: Int,
        userId: String
    ): Boolean {
        if (getRefreshToken() != expectedRefreshToken) return false
        writeTokens(accessToken, refreshToken, expiresIn, userId)
        return true
    }

    private fun writeTokens(
        accessToken: String,
        refreshToken: String?,
        expiresIn: Int,
        userId: String
    ) {
        val expiresAt = System.currentTimeMillis() + expiresIn * 1000L
        val success = prefs.edit()
            .putString(KEY_ACCESS_TOKEN, accessToken)
            .apply {
                if (refreshToken != null) putString(KEY_REFRESH_TOKEN, refreshToken)
                else remove(KEY_REFRESH_TOKEN)
            }
            .putLong(KEY_EXPIRES_AT, expiresAt)
            .putString(KEY_USER_ID, userId)
            .commit()
        if (!success) {
            logger.warning("Failed to persist tokens to storage")
        }
    }

    fun getAccessToken(): String? = prefs.getString(KEY_ACCESS_TOKEN, null)

    fun getRefreshToken(): String? {
        val token = prefs.getString(KEY_REFRESH_TOKEN, null)
        return if (token.isNullOrEmpty()) null else token
    }

    fun getUserId(): String? = prefs.getString(KEY_USER_ID, null)

    fun getServerUrl(): String? = prefs.getString(KEY_SERVER_URL, null)

    @Synchronized
    fun saveServerUrl(url: String) {
        sessionGeneration++
        val success = prefs.edit()
            .putString(KEY_SERVER_URL, url)
            .commit()
        if (!success) {
            logger.warning("Failed to persist server URL to storage")
        }
    }

    /** Epoch millis at which the stored access token expires, or 0 when none is stored. */
    fun getAccessTokenExpiresAtMillis(): Long = prefs.getLong(KEY_EXPIRES_AT, 0L)

    fun isAccessTokenExpired(): Boolean {
        val expiresAt = prefs.getLong(KEY_EXPIRES_AT, 0L)
        return System.currentTimeMillis() >= expiresAt
    }

    fun saveOidcPkceState(codeVerifier: String, state: String) {
        val success = prefs.edit()
            .putString(KEY_OIDC_CODE_VERIFIER, codeVerifier)
            .putString(KEY_OIDC_STATE, state)
            .commit()
        if (!success) {
            logger.warning("Failed to persist OIDC PKCE state to storage")
        }
    }

    fun getOidcCodeVerifier(): String? = prefs.getString(KEY_OIDC_CODE_VERIFIER, null)

    fun getOidcState(): String? = prefs.getString(KEY_OIDC_STATE, null)

    fun clearOidcPkceState() {
        val success = prefs.edit()
            .remove(KEY_OIDC_CODE_VERIFIER)
            .remove(KEY_OIDC_STATE)
            .commit()
        if (!success) {
            logger.warning("Failed to clear OIDC PKCE state from storage")
        }
    }

    /** Removes the session tokens (logout). Starts a new session generation. */
    @Synchronized
    fun clear() {
        sessionGeneration++
        removeTokens()
    }

    /**
     * Removes the session tokens only if the stored refresh token is still
     * [expectedRefreshToken] (null meaning "none stored"), so a rejected refresh
     * of an old session never wipes a newer login.
     *
     * @return true if the tokens were removed (or already absent as expected).
     */
    @Synchronized
    fun clearIfRefreshTokenIs(expectedRefreshToken: String?): Boolean {
        if (getRefreshToken() != expectedRefreshToken) return false
        sessionGeneration++
        removeTokens()
        return true
    }

    private fun removeTokens() {
        val success = prefs.edit()
            .remove(KEY_ACCESS_TOKEN)
            .remove(KEY_REFRESH_TOKEN)
            .remove(KEY_EXPIRES_AT)
            .remove(KEY_USER_ID)
            .commit()
        if (!success) {
            logger.warning("Failed to clear tokens from storage")
        }
    }
}
