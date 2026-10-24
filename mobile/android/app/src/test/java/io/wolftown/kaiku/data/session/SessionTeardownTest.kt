package io.wolftown.kaiku.data.session

import io.mockk.coEvery
import io.mockk.every
import io.mockk.mockk
import io.mockk.verifyOrder
import io.wolftown.kaiku.data.KaikuJson
import io.wolftown.kaiku.data.api.ChannelApi
import io.wolftown.kaiku.data.api.DmApi
import io.wolftown.kaiku.data.api.GuildApi
import io.wolftown.kaiku.data.local.AuthSession
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.data.repository.ChatRepository
import io.wolftown.kaiku.data.repository.DmRepository
import io.wolftown.kaiku.data.repository.GuildRepository
import io.wolftown.kaiku.data.sync.ResyncCoordinator
import io.wolftown.kaiku.data.ws.KaikuWebSocket
import io.wolftown.kaiku.data.ws.ServerEvent
import io.wolftown.kaiku.domain.model.Channel
import io.wolftown.kaiku.domain.model.ChannelType
import io.wolftown.kaiku.domain.model.DmConversation
import io.wolftown.kaiku.domain.model.Guild
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** The session teardown runs once per ended session, whoever ended it. */
class SessionTeardownTest {

    private val guildApi: GuildApi = mockk(relaxed = true) {
        coEvery { getGuilds() } returns listOf(Guild(id = "g1", name = "Family"))
    }
    private val channelApi: ChannelApi = mockk(relaxed = true) {
        coEvery { getChannels("g1") } returns listOf(
            Channel(id = "c1", name = "general", channelType = ChannelType.TEXT, unreadCount = 2)
        )
    }
    private val dmApi: DmApi = mockk(relaxed = true) {
        coEvery { getDms() } returns listOf(DmConversation(id = "dm-1", unreadCount = 1))
    }
    private val webSocket: KaikuWebSocket = mockk(relaxed = true) {
        every { events } returns MutableSharedFlow<ServerEvent>()
    }
    private val chat: ChatRepository = mockk(relaxed = true)
    private val resync: ResyncCoordinator = mockk(relaxed = true)

    private class Counting(override val order: Int) : SessionTeardown {
        var calls = 0
        override suspend fun onLogout() {
            calls++
        }
    }

    private fun TestScope.loadedRepositories(authState: AuthState): Pair<GuildRepository, DmRepository> {
        val guilds = GuildRepository(guildApi, channelApi, backgroundScope)
        val dms = DmRepository(dmApi, webSocket, KaikuJson, authState, backgroundScope)
        return guilds to dms
    }

    @Test
    fun `auth rejected logout clears repositories`() = runTest {
        val authState = AuthState(backgroundScope)
        authState.setLoggedIn("user-1")
        val (guilds, dms) = loadedRepositories(authState)
        guilds.loadGuilds()
        guilds.selectGuild("g1")
        guilds.loadChannels("g1")
        guilds.setActiveChannel("c1")
        dms.loadDms()
        dms.setActiveChannel("dm-1")
        val runner = SessionTeardownRunner(
            setOf(CacheSessionTeardown(resync, chat, guilds, dms)),
            authState,
            backgroundScope,
        )
        runner.start()
        runCurrent()

        // What KaikuHttpClient / RealtimeConnectionManager do on a rejected
        // session: clear the tokens and set LoggedOut. Nothing else.
        authState.setLoggedOut()
        runCurrent()

        assertTrue(guilds.guilds.value.isEmpty())
        assertTrue(guilds.channels.value.isEmpty())
        assertTrue(guilds.categories.value.isEmpty())
        assertNull(guilds.selectedGuildId.value)
        assertTrue(dms.conversations.value.isEmpty())
        verifyOrder {
            resync.cancelForLogout()
            chat.clearSession()
        }
    }

    @Test
    fun `rejected session tears down once`() = runTest {
        val authState = AuthState(backgroundScope)
        authState.setLoggedIn("user-1")
        val session = authState.session.value as AuthSession.LoggedIn
        val step = Counting(10)
        val runner = SessionTeardownRunner(setOf(step), authState, backgroundScope)
        runner.start()
        runCurrent()

        authState.setLoggedOut() // rejection: the observer tears down
        runCurrent()
        runner.tearDown(session) // an explicit logout of the same session that was in flight
        runCurrent()

        assertEquals(1, step.calls)
    }

    @Test
    fun `same session is not torn down twice`() = runTest {
        val authState = AuthState(backgroundScope)
        authState.setLoggedIn("user-1")
        val session = authState.session.value as AuthSession.LoggedIn
        val step = Counting(10)
        val runner = SessionTeardownRunner(setOf(step), authState, backgroundScope)
        runner.start()
        runCurrent()

        runner.tearDown(session) // explicit logout claims the session
        authState.setLoggedOut() // ...and then ends in LoggedOut
        runCurrent()

        assertEquals(1, step.calls)
    }

    @Test
    fun `teardown stops when a newer login took over`() = runTest {
        val authState = AuthState(backgroundScope)
        authState.setLoggedIn("user-1")
        val old = authState.session.value as AuthSession.LoggedIn
        val first = object : SessionTeardown {
            override val order = 10
            override suspend fun onLogout() {
                authState.setLoggedOut()
                authState.setLoggedIn("user-2") // a new login lands mid-teardown
            }
        }
        val second = Counting(30)
        val runner = SessionTeardownRunner(setOf(first, second), authState, backgroundScope)

        runner.tearDown(old)

        assertEquals(0, second.calls)
    }

    @Test
    fun `logged out at start does not trigger a teardown`() = runTest {
        val authState = AuthState(backgroundScope)
        val step = Counting(10)
        val runner = SessionTeardownRunner(setOf(step), authState, backgroundScope)
        runner.start()
        runCurrent()

        authState.setLoggedOut()
        runCurrent()

        assertEquals(0, step.calls)
    }
}
