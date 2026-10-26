package io.wolftown.kaiku.navigation

import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.getAndUpdate
import javax.inject.Inject
import javax.inject.Singleton

/** A channel to open once the user is signed in (deep link or notification tap). */
@Singleton
class PendingNavigation @Inject constructor() {
    private val _channelId = MutableStateFlow<String?>(null)
    val channelId: StateFlow<String?> = _channelId.asStateFlow()

    /** User id signed in when the link was posted; null when signed out. */
    @Volatile
    var userTag: String? = null
        private set

    fun post(channelId: String, userId: String? = null) {
        userTag = userId
        _channelId.value = channelId
    }

    /** Returns the pending channel id and clears it. */
    fun consume(): String? = _channelId.getAndUpdate { null }

    fun clear() {
        userTag = null
        _channelId.value = null
    }
}

/** Pure rules for when a pending link may be applied or must be dropped. */
object PendingNavigationRules {
    private fun routeReady(route: String?, authRoutes: Set<String>) =
        route != null && route !in authRoutes

    private fun tagMatches(pendingTag: String?, currentUserId: String?) =
        pendingTag == null || pendingTag == currentUserId

    fun shouldConsume(
        isLoggedIn: Boolean,
        pendingTag: String?,
        currentUserId: String?,
        route: String?,
        authRoutes: Set<String>,
    ): Boolean = isLoggedIn && routeReady(route, authRoutes) && tagMatches(pendingTag, currentUserId)

    /** A link posted for another account must never be applied to this one. */
    fun shouldDrop(
        isLoggedIn: Boolean,
        pendingTag: String?,
        currentUserId: String?,
    ): Boolean = isLoggedIn && currentUserId != null && !tagMatches(pendingTag, currentUserId)
}
