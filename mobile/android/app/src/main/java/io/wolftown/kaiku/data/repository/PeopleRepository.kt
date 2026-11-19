package io.wolftown.kaiku.data.repository

import io.wolftown.kaiku.data.api.DmApi
import io.wolftown.kaiku.data.api.GuildApi
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.domain.Person
import io.wolftown.kaiku.domain.messageCandidates
import io.wolftown.kaiku.domain.model.GuildMember
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.async
import kotlinx.coroutines.awaitAll
import kotlinx.coroutines.coroutineScope
import java.util.logging.Logger
import javax.inject.Inject
import javax.inject.Singleton

/** The people offered by "New message", and how many servers could not be read. */
data class People(val people: List<Person>, val failedServers: Int)

/** Every server's member list failed to load (offline, server error). */
class PeopleUnavailableException(cause: Throwable?) : Exception("no member list could be loaded", cause)

/**
 * Loads who the user can write to: all members of the user's servers,
 * fresh on every call (people join and leave), the same rule as the web
 * client. No friendship is needed.
 */
@Singleton
class PeopleRepository @Inject constructor(
    private val guildApi: GuildApi,
    private val dmApi: DmApi,
    private val authState: AuthState,
) {
    /**
     * Member lists load in parallel. One server failing only counts in
     * [People.failedServers]; all of them failing throws
     * [PeopleUnavailableException]. The block list is best effort: without it
     * nobody is hidden and the server still refuses a blocked pair.
     */
    suspend fun load(): People = coroutineScope {
        val blocked = async { loadBlockedQuietly() }
        val guilds = guildApi.getGuilds()
        val lists = guilds.map { g -> async { loadMembers(g.id) } }.awaitAll()
        val loaded = lists.mapNotNull { it.getOrNull() }
        if (guilds.isNotEmpty() && loaded.isEmpty()) {
            throw PeopleUnavailableException(lists.firstNotNullOfOrNull { it.exceptionOrNull() })
        }
        People(
            people = messageCandidates(loaded, authState.currentUserId.value, blocked.await()),
            failedServers = lists.size - loaded.size,
        )
    }

    private suspend fun loadMembers(guildId: String): Result<List<GuildMember>> = try {
        Result.success(guildApi.getMembers(guildId))
    } catch (e: CancellationException) {
        throw e
    } catch (e: Exception) {
        logger.warning("member list failed (${e::class.simpleName})")
        Result.failure(e)
    }

    private suspend fun loadBlockedQuietly(): Set<String> = try {
        dmApi.getBlocked()
    } catch (e: CancellationException) {
        throw e
    } catch (e: Exception) {
        logger.warning("block list failed (${e::class.simpleName})")
        emptySet()
    }

    private companion object {
        val logger: Logger = Logger.getLogger("PeopleRepository")
    }
}
