package io.wolftown.kaiku.data.auth

import io.wolftown.kaiku.data.api.AuthApi
import io.wolftown.kaiku.data.local.AuthState
import javax.inject.Inject
import javax.inject.Singleton
import kotlin.coroutines.cancellation.CancellationException

/**
 * `may_approve` of the signed-in account (C27 `GET /auth/me`), cached so the
 * scanner can refuse a sign-in code without a request (Q4). Null = not known
 * yet; the server re-checks at claim and approve anyway.
 */
@Singleton
class AccountFlags @Inject constructor(
    private val authApi: AuthApi,
    private val authState: AuthState,
) {
    @Volatile
    private var cached: Pair<String, Boolean>? = null

    fun mayApprove(): Boolean? {
        val user = authState.currentUserId.value ?: return null
        return cached?.takeIf { it.first == user }?.second
    }

    suspend fun refresh() {
        if (!authState.isLoggedIn.value) return
        try {
            val me = authApi.getMe()
            cached = me.id to me.mayApprove
        } catch (e: CancellationException) {
            throw e
        } catch (_: Exception) {
            // Keep the last value; the server decides at claim.
        }
    }
}
