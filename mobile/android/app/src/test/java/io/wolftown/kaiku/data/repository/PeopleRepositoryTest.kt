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
    private val repo = PeopleRepository(guildApi, dmApi, authState)

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
}
