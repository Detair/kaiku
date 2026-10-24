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

    fun post(channelId: String) {
        _channelId.value = channelId
    }

    /** Returns the pending channel id and clears it. */
    fun consume(): String? = _channelId.getAndUpdate { null }

    fun clear() {
        _channelId.value = null
    }
}
