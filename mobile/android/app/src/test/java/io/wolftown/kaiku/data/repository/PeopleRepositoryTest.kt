package io.wolftown.kaiku.data.repository

import io.ktor.http.HttpStatusCode
import io.mockk.coEvery
import io.mockk.every
import io.mockk.mockk
import io.wolftown.kaiku.data.api.ApiException
import io.wolftown.kaiku.data.api.DmApi
import io.wolftown.kaiku.data.api.GuildApi
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.domain.model.Guild
import io.wolftown.kaiku.domain.model.GuildMember
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.async
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test
import java.io.IOException

class PeopleRepositoryTest {
    private val guildApi: GuildApi = mockk()
    private val dmApi: DmApi = mockk()
    private val authState: AuthState = mockk { every { currentUserId } returns MutableStateFlow<String?>("me") }
    private val sessions = io.wolftown.kaiku.data.session.FakeSessions("me")
    private val events = kotlinx.coroutines.flow.MutableSharedFlow<io.wolftown.kaiku.data.ws.ServerEvent>(extraBufferCapacity = 8)
    private val socket: io.wolftown.kaiku.data.ws.KaikuWebSocket = mockk(relaxed = true) { every { events } returns this@PeopleRepositoryTest.events }
    private val scope = kotlinx.coroutines.CoroutineScope(kotlinx.coroutines.SupervisorJob() + kotlinx.coroutines.test.UnconfinedTestDispatcher())
    private val repo = PeopleRepository(guildApi, dmApi, authState, socket, scope, sessions.generation)

    private fun member(id: String) = GuildMember(userId = id, username = id, displayName = id.uppercase())

    @Test
    fun `one server failing still shows the others and counts it`() = runTest {
        coEvery { guildApi.getGuilds() } returns listOf(Guild("g1", "Family"), Guild("g2", "Old"))
        coEvery { guildApi.getMembers("g1") } returns listOf(member("me"), member("anna"))
        coEvery { guildApi.getMembers("g2") } throws ApiException(HttpStatusCode.Forbidden, "x", code = "FORBIDDEN")
        coEvery { dmApi.getBlocked() } returns emptySet()

        val result = repo.load()

        assertEquals(listOf("anna"), result.people.map { it.userId })
        assertEquals(1, result.failedServers)
    }

    @Test
    fun `all servers failing is an error`() = runTest {
        coEvery { guildApi.getGuilds() } returns listOf(Guild("g1", "Family"))
        coEvery { guildApi.getMembers("g1") } throws IOException("offline")
        coEvery { dmApi.getBlocked() } returns emptySet()

        try {
            repo.load()
            fail("expected an error")
        } catch (e: PeopleUnavailableException) {
            assertTrue(generateSequence<Throwable>(e) { it.cause }.any { it is IOException })
        }
    }

    @Test
    fun `no servers is an empty list, not an error`() = runTest {
        coEvery { guildApi.getGuilds() } returns emptyList()
        coEvery { dmApi.getBlocked() } returns emptySet()
        assertEquals(People(emptyList(), 0), repo.load())
    }

    @Test
    fun `a failed block list offers nobody, a loaded one hides the blocked`() = runTest {
        coEvery { guildApi.getGuilds() } returns listOf(Guild("g1", "Family"))
        coEvery { guildApi.getMembers("g1") } returns listOf(member("anna"), member("troll"))
        coEvery { dmApi.getBlocked() } throws IOException("x")
        // Never offer someone who may be blocked: the whole list fails, with Retry.
        try {
            repo.load()
            org.junit.Assert.fail("expected PeopleUnavailableException")
        } catch (_: PeopleUnavailableException) {
        }

        coEvery { dmApi.getBlocked() } returns setOf("troll")
        assertEquals(listOf("anna"), repo.load().people.map { it.userId })
    }

    // -- Who can be written to, checked again -------------------------------------------------

    @Test
    fun `someone still on a shared server and not blocked is confirmed`() = runTest {
        coEvery { guildApi.getGuilds() } returns listOf(Guild("g1", "Family"))
        coEvery { guildApi.getMembers("g1") } returns listOf(member("me"), member("anna"))
        coEvery { dmApi.getBlocked() } returns emptySet()
        assertEquals("anna", repo.confirm("anna")?.userId)
    }

    @Test
    fun `someone whose server the user has left since is not confirmed`() = runTest {
        // The list was shown with Anna (server g1); the user left g1 on another device.
        coEvery { guildApi.getGuilds() } returns emptyList()
        coEvery { dmApi.getBlocked() } returns emptySet()
        assertEquals(null, repo.confirm("anna"))
    }

    @Test
    fun `someone not found while a server's member list failed is not refused but could not be checked`() = runTest {
        coEvery { guildApi.getGuilds() } returns listOf(Guild("g1", "Family"), Guild("g2", "Club"))
        coEvery { guildApi.getMembers("g1") } returns listOf(member("me"), member("bert"))
        // Anna is on g2, whose member list does not load right now.
        coEvery { guildApi.getMembers("g2") } throws IOException("offline")
        coEvery { dmApi.getBlocked() } returns emptySet()
        try {
            repo.confirm("anna")
            fail("a person on a server that could not be read was refused")
        } catch (e: PeopleUnavailableException) {
            assertTrue(generateSequence<Throwable>(e) { it.cause }.any { it is IOException })
        }
        // Someone found is confirmed all the same.
        assertEquals("bert", repo.confirm("bert")?.userId)
    }

    @Test
    fun `someone blocked since on another device is not confirmed`() = runTest {
        coEvery { guildApi.getGuilds() } returns listOf(Guild("g1", "Family"))
        coEvery { guildApi.getMembers("g1") } returns listOf(member("me"), member("anna"))
        coEvery { dmApi.getBlocked() } returns setOf("anna")
        assertEquals(null, repo.confirm("anna"))
    }

    @Test
    fun `a block or a new connection says the list may have changed`() = runTest {
        val seen = mutableListOf<Unit>()
        val job = kotlinx.coroutines.CoroutineScope(kotlinx.coroutines.test.UnconfinedTestDispatcher(testScheduler)).launch {
            repo.invalidated.collect { seen += it }
        }
        events.emit(io.wolftown.kaiku.data.ws.ServerEvent.UserBlocked("anna"))
        events.emit(io.wolftown.kaiku.data.ws.ServerEvent.UserUnblocked("anna"))
        events.emit(io.wolftown.kaiku.data.ws.ServerEvent.Ready("me"))
        events.emit(io.wolftown.kaiku.data.ws.ServerEvent.Pong)
        assertEquals(3, seen.size)
        job.cancel()
    }

    @Test
    fun `a block names who was blocked`() = runTest {
        val seen = mutableListOf<String>()
        val job = kotlinx.coroutines.CoroutineScope(kotlinx.coroutines.test.UnconfinedTestDispatcher(testScheduler)).launch {
            repo.blocked.collect { seen += it }
        }
        events.emit(io.wolftown.kaiku.data.ws.ServerEvent.UserBlocked("anna"))
        events.emit(io.wolftown.kaiku.data.ws.ServerEvent.UserUnblocked("bert"))
        assertEquals(listOf("anna"), seen)
        job.cancel()
    }

    @Test
    fun `nobody signed in loads nobody, and a list of the previous account is not handed out`() = runTest {
        val members = kotlinx.coroutines.CompletableDeferred<List<GuildMember>>()
        coEvery { guildApi.getGuilds() } returns listOf(Guild("g1", "Family"))
        coEvery { guildApi.getMembers("g1") } coAnswers { members.await() }
        coEvery { dmApi.getBlocked() } returns emptySet()
        val load = async { runCatching { repo.load() } }
        kotlinx.coroutines.yield()
        // The account changes while the member list is on its way.
        sessions.switchTo("bert")
        members.complete(listOf(member("me"), member("anna")))
        assertTrue(load.await().isFailure)

        sessions.signOut()
        assertTrue(runCatching { repo.load() }.isFailure)
    }
}
