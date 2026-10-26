package io.wolftown.kaiku.push

import io.wolftown.kaiku.data.api.ChannelApi
import io.wolftown.kaiku.data.api.DmApi
import io.wolftown.kaiku.data.local.TokenStorage
import io.wolftown.kaiku.data.repository.DmRepository
import io.wolftown.kaiku.domain.model.ChannelType
import kotlinx.coroutines.CancellationException
import javax.inject.Inject

/** Resolves a notification title over REST: the DM title, or `#name` for a server channel. */
class NotificationTitleResolver @Inject constructor(
    private val dmApi: DmApi,
    private val channelApi: ChannelApi,
    private val tokenStorage: TokenStorage,
) {
    /**
     * Returns (title, isDm), or null on any failure. [preferDm] tries the DM list
     * first (one request for DM wakes); otherwise the channel is looked up first.
     */
    suspend fun resolve(channelId: String, preferDm: Boolean = false): Pair<String, Boolean>? =
        try {
            if (preferDm) {
                dmTitle(channelId)?.let { it to true } ?: channelTitle(channelId)
            } else {
                channelTitle(channelId)
            }
        } catch (e: CancellationException) {
            throw e
        } catch (_: Exception) {
            null
        }

    private suspend fun channelTitle(channelId: String): Pair<String, Boolean>? {
        val channel = channelApi.getChannel(channelId)
        return if (channel.channelType == ChannelType.DM) {
            dmTitle(channelId)?.let { it to true }
        } else {
            "#${channel.name}" to false
        }
    }

    private suspend fun dmTitle(channelId: String): String? {
        val dm = dmApi.getDms().find { it.id == channelId } ?: return null
        return DmRepository.conversationTitle(dm, tokenStorage.getUserId().orEmpty())
    }
}
