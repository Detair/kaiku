package io.wolftown.kaiku.data.repository

import io.wolftown.kaiku.data.api.DmApi
import io.wolftown.kaiku.data.api.GuildApi
import io.wolftown.kaiku.data.api.withRequestSession
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.data.session.SessionGeneration
import io.wolftown.kaiku.data.ws.KaikuWebSocket
import io.wolftown.kaiku.data.ws.ServerEvent
import io.wolftown.kaiku.di.ChatCoroutineScope
import io.wolftown.kaiku.domain.Person
import io.wolftown.kaiku.domain.messageCandidates
import io.wolftown.kaiku.domain.model.GuildMember
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.async
import kotlinx.coroutines.awaitAll
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.launch
import java.util.logging.Logger
import javax.inject.Inject
import javax.inject.Singleton

/**
 * The people offered by "New message", and how many servers could not be read
 * ([failure]: why the first of them could not).
 */
data class People(val people: List<Person>, val failedServers: Int, val failure: Throwable? = null)

/** Every server's member list failed to load (offline, server error). */
class PeopleUnavailableException(cause: Throwable?) : Exception("no member list could be loaded", cause)

/** Nobody is signed in (any more): there is nobody to load people for. */
class NotSignedInException : CancellationException("nobody is signed in")

/**
 * Someone the signed-in user may open a conversation with right now: a
 * fresh look at the servers' members and the block list has just confirmed
 * it. Only [PeopleRepository.confirm] makes one, and it is good for the
 * signed-in session it was made in.
 */
class EligiblePerson internal constructor(
    val userId: String,
    internal val session: SessionGeneration.Ticket,
)

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
    webSocket: KaikuWebSocket,
    @ChatCoroutineScope scope: CoroutineScope,
    private val sessions: SessionGeneration = SessionGeneration(authState),
) {
    private val _invalidated = MutableSharedFlow<Unit>(extraBufferCapacity = 1)

    /**
     * Who can be written to may have changed: this user blocked or
     * unblocked someone (on any device), or the connection was down and is
     * back (anything may have changed meanwhile). A list that is shown is
     * loaded again.
     */
    val invalidated: SharedFlow<Unit> = _invalidated.asSharedFlow()

    init {
        // UNDISPATCHED: subscribed before init returns (the events have no replay).
        scope.launch(start = CoroutineStart.UNDISPATCHED) {
            webSocket.events.collect { event ->
                when (event) {
                    is ServerEvent.UserBlocked, is ServerEvent.UserUnblocked, is ServerEvent.Ready -> _invalidated.tryEmit(Unit)
                    else -> Unit
                }
            }
        }
    }

    /**
     * Member lists load in parallel. One server failing only counts in
     * [People.failedServers]; all of them failing throws
     * [PeopleUnavailableException]. So does a block list that cannot be
     * loaded: nobody who may be blocked is ever offered.
     *
     * The requests belong to the signed-in session of this moment: once it
     * is over (or being signed out) they are not sent, and an answer that
     * arrives later is not returned ([NotSignedInException]).
     */
    suspend fun load(): People = loadFor(openSession())

    /**
     * Whether [userId] can still be written to, by a fresh [load]: someone
     * who left the user's servers or was blocked since the list was shown
     * (on this or another device) is not confirmed (null). Throws what
     * [load] throws when it cannot be checked, and
     * [PeopleUnavailableException] when the person is not among those
     * loaded while a server's member list could not be loaded: they may be
     * on that server, so that is no answer.
     */
    suspend fun confirm(userId: String): EligiblePerson? {
        val session = openSession()
        val now = loadFor(session)
        if (now.people.any { it.userId == userId }) return EligiblePerson(userId, session)
        if (now.failedServers > 0) throw PeopleUnavailableException(now.failure)
        return null
    }

    private fun openSession(): SessionGeneration.Ticket =
        sessions.enter()?.takeIf { it.open } ?: throw NotSignedInException()

    private suspend fun loadFor(session: SessionGeneration.Ticket): People {
        val people = withRequestSession(session.requestSession()) {
            coroutineScope {
                val blocked = async { loadBlocked() }
                val guilds = guildApi.getGuilds()
                val lists = guilds.map { g -> async { loadMembers(g.id) } }.awaitAll()
                val loaded = lists.mapNotNull { it.getOrNull() }
                if (guilds.isNotEmpty() && loaded.isEmpty()) {
                    throw PeopleUnavailableException(lists.firstNotNullOfOrNull { it.exceptionOrNull() })
                }
                People(
                    people = messageCandidates(loaded, session.userId, blocked.await()),
                    failedServers = lists.size - loaded.size,
                    failure = lists.firstNotNullOfOrNull { it.exceptionOrNull() },
                )
            }
        }
        // Another account's people are nobody's to see.
        if (!session.open) throw NotSignedInException()
        return people
    }

    private suspend fun loadMembers(guildId: String): Result<List<GuildMember>> = try {
        Result.success(guildApi.getMembers(guildId))
    } catch (e: CancellationException) {
        throw e
    } catch (e: Exception) {
        logger.warning("member list failed (${e::class.simpleName})")
        Result.failure(e)
    }

    private suspend fun loadBlocked(): Set<String> = try {
        dmApi.getBlocked()
    } catch (e: CancellationException) {
        throw e
    } catch (e: Exception) {
        logger.warning("block list failed (${e::class.simpleName})")
        throw PeopleUnavailableException(e)
    }

    private companion object {
        val logger: Logger = Logger.getLogger("PeopleRepository")
    }
}
