package io.wolftown.kaiku.data.auth

/** Outcome of a token refresh attempt. */
enum class RefreshResult {
    /** A usable access token is stored (refreshed now, or already fresh / rotated by another caller). */
    SUCCESS,

    /** The server rejected the refresh token; the session has been ended. */
    AUTH_REJECTED,

    /** Transient failure (no connectivity, timeout, 5xx); the session is kept. */
    NETWORK_ERROR,

    /** No refresh token is stored; the session has been ended. */
    NO_SESSION,
}

/**
 * Keeps the stored access token usable.
 *
 * All refreshes (proactive, forced and the transparent 401 path of the HTTP
 * client) are serialized through one lock, so concurrent callers trigger at most
 * one `/auth/refresh` request. [RefreshResult.AUTH_REJECTED] and
 * [RefreshResult.NO_SESSION] log the user out and clear stored tokens.
 */
interface TokenRefresher {
    /**
     * Returns [RefreshResult.SUCCESS] without a network call when the access token
     * stays valid for at least [minValidityMs]; otherwise refreshes it.
     */
    suspend fun ensureFresh(minValidityMs: Long = 60_000): RefreshResult

    /**
     * Refreshes the access token regardless of its expiry, unless another caller
     * already rotated it while this call waited for the lock.
     */
    suspend fun forceRefresh(): RefreshResult
}
