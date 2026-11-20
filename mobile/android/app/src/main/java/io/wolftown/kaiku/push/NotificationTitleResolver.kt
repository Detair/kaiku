package io.wolftown.kaiku.push

import io.ktor.http.HttpStatusCode
import io.wolftown.kaiku.data.api.ApiException
import io.wolftown.kaiku.data.api.ChannelApi
import io.wolftown.kaiku.data.api.DmApi
import io.wolftown.kaiku.data.local.TokenStorage
import io.wolftown.kaiku.data.repository.DmRepository
import io.wolftown.kaiku.domain.model.ChannelType
import io.wolftown.kaiku.domain.model.DmConversation
import kotlinx.coroutines.CancellationException
import javax.inject.Inject

/** Whether a DM belongs to the signed-in user, and the caller's name in it. */
sealed interface CallerLookup {
    /** The DM is the user's; [name] is null when the caller has no usable name in it. */
    data class Found(val name: String?) : CallerLookup

    /** The loaded DM list does not have the channel, or the server answered 403/404: not the user's DM. */
    data object NotMember : CallerLookup

    /** Could not be decided (network or server error). */
    data object Unknown : CallerLookup
}

/**
 * Resolves a notification title over REST: the DM title, or `#name` for a
 * server channel; and a caller's name from the DM participants.
 */
class NotificationTitleResolver @Inject constructor(
    private val dmApi: DmApi,
    private val channelApi: ChannelApi,
    private val tokenStorage: TokenStorage,
    private val dmRepository: DmRepository,
) {
    /**
     * Looks up DM [channelId] and [callerId]'s display name in it: from the
     * cached DM list when it has the caller, otherwise from the DM list over
     * REST, which also decides membership ([CallerLookup.NotMember]).
     */
    suspend fun lookupCaller(channelId: String, callerId: String): CallerLookup {
        participantName(dmRepository.conversations.value.find { it.id == channelId }, callerId)
            ?.let { return CallerLookup.Found(it) }
        return try {
            val dm = dmApi.getDms().find { it.id == channelId } ?: return CallerLookup.NotMember
            CallerLookup.Found(participantName(dm, callerId))
        } catch (e: CancellationException) {
            throw e
        } catch (e: ApiException) {
            if (e.status == HttpStatusCode.Forbidden || e.status == HttpStatusCode.NotFound) {
                CallerLookup.NotMember
            } else {
                CallerLookup.Unknown
            }
        } catch (_: Exception) {
            CallerLookup.Unknown
        }
    }

    private fun participantName(dm: DmConversation?, userId: String): String? {
        val participant = dm?.participants?.find { it.userId == userId } ?: return null
        return participant.displayName.ifBlank { participant.username }.takeIf { it.isNotBlank() }
    }

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
