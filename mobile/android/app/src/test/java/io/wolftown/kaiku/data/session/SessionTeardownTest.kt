package io.wolftown.kaiku.data.session

import io.wolftown.kaiku.testutil.appScope
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
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
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
            appScope(),
        )
        runner.start()
        runCurrent()

        // What KaikuHttpClient / RealtimeConnectionManager do on a rejected
        // session: clear the tokens and set LoggedOut. Nothing else.
        authState.setLoggedOut()
        advanceUntilIdle()

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
        val step = Counting(10)
        val runner = SessionTeardownRunner(setOf(step), authState, appScope())
        runner.start()
        authState.setLoggedIn("user-1")
        val session = authState.session.value as AuthSession.LoggedIn

        authState.setLoggedOut() // rejection: the observer tears down
        advanceUntilIdle()
        runner.tearDown(session.sessionId).join() // an explicit logout of the same session that was in flight

        assertEquals(1, step.calls)
    }

    @Test
    fun `same session is not torn down twice`() = runTest {
        val authState = AuthState(backgroundScope)
        val step = Counting(10)
        val runner = SessionTeardownRunner(setOf(step), authState, appScope())
        runner.start()
        authState.setLoggedIn("user-1")
        val session = authState.session.value as AuthSession.LoggedIn

        val explicit = runner.tearDown(session.sessionId) // explicit logout starts it
        authState.setLoggedOut() // ...and then ends in LoggedOut
        advanceUntilIdle()

        assertTrue(explicit.isCompleted)
        assertEquals(1, step.calls)
    }

    @Test
    fun `quick logins and logouts tear down every session`() = runTest {
        val authState = AuthState(backgroundScope)
        val step = Counting(10)
        val runner = SessionTeardownRunner(setOf(step), authState, appScope())
        runner.start()

        // All before the observer resumes: a StateFlow would conflate them.
        authState.setLoggedIn("user-a")
        authState.setLoggedOut()
        authState.setLoggedIn("user-b")
        authState.setLoggedOut()
        advanceUntilIdle()

        assertEquals(2, step.calls)
    }

    @Test
    fun `same user re-login during teardown is not torn down`() = runTest {
        val authState = AuthState(backgroundScope)
        authState.setLoggedIn("user-1")
        val old = authState.session.value as AuthSession.LoggedIn
        val first = object : SessionTeardown {
            override val order = 10
            override suspend fun onLogout() {
                authState.setLoggedOut()
                authState.setLoggedIn("user-1") // the same user signs in again mid-teardown
            }
        }
        val second = Counting(30)
        val runner = SessionTeardownRunner(setOf(first, second), authState, appScope())
        runner.start()

        runner.tearDown(old.sessionId).join()
        advanceUntilIdle()

        val now = authState.session.value as AuthSession.LoggedIn
        assertEquals("user-1", now.userId)
        assertNotEquals(old.sessionId, now.sessionId)
        // The rest of the old teardown was skipped; the new session was never torn down.
        assertEquals(0, second.calls)
    }

    @Test
    fun `teardown cancelled abnormally runs again on the next trigger`() = runTest {
        val authState = AuthState(backgroundScope)
        authState.setLoggedIn("user-1")
        val session = authState.session.value as AuthSession.LoggedIn
        val gate = CompletableDeferred<Unit>()
        var starts = 0
        val step = object : SessionTeardown {
            override val order = 10
            override suspend fun onLogout() {
                starts++
                if (starts == 1) gate.await()
            }
        }
        val runner = SessionTeardownRunner(setOf(step), authState, appScope())

        val first = runner.tearDown(session.sessionId)
        runCurrent()
        first.cancel()
        runCurrent()
        runner.tearDown(session.sessionId).join()

        assertEquals(2, starts)
    }

    @Test
    fun `logged out at start does not trigger a teardown`() = runTest {
        val authState = AuthState(backgroundScope)
        val step = Counting(10)
        val runner = SessionTeardownRunner(setOf(step), authState, appScope())
        runner.start()

        authState.setLoggedOut()
        advanceUntilIdle()

        assertEquals(0, step.calls)
    }
}
