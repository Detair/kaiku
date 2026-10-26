package io.wolftown.kaiku.push

import io.ktor.http.HttpStatusCode
import io.mockk.coEvery
import io.mockk.coVerify
import io.mockk.every
import io.mockk.mockk
import io.wolftown.kaiku.data.api.ApiException
import io.wolftown.kaiku.data.api.ChannelApi
import io.wolftown.kaiku.data.api.DmApi
import io.wolftown.kaiku.data.local.TokenStorage
import io.wolftown.kaiku.data.repository.DmRepository
import io.wolftown.kaiku.domain.model.DmConversation
import io.wolftown.kaiku.domain.model.DmParticipant
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.test.runTest
import java.io.IOException
import org.junit.Assert.assertEquals
import org.junit.Before
import org.junit.Test

class NotificationTitleResolverTest {

    private val ch = "0f8fad5b-d9cb-469f-a165-70867728950e"
    private val bob = "7a8b9c0d-1e2f-4a3b-8c4d-5e6f7a8b9c0d"

    private val dmApi: DmApi = mockk()
    private val channelApi: ChannelApi = mockk()
    private val tokenStorage: TokenStorage = mockk()
    private val dmRepository: DmRepository = mockk()
    private val cache = MutableStateFlow<List<DmConversation>>(emptyList())
    private val resolver = NotificationTitleResolver(dmApi, channelApi, tokenStorage, dmRepository)

    private fun dm(vararg participants: DmParticipant) = DmConversation(id = ch, participants = participants.toList())

    private val bobParticipant = DmParticipant(userId = bob, username = "bob", displayName = "Bob")
    private val meParticipant = DmParticipant(userId = "me", username = "me", displayName = "Me")

    @Before
    fun setUp() {
        every { dmRepository.conversations } returns cache
        every { tokenStorage.getUserId() } returns "me"
    }

    @Test
    fun `caller in the cached DM is found without a request`() = runTest {
        cache.value = listOf(dm(meParticipant, bobParticipant))

        assertEquals(CallerLookup.Found("Bob"), resolver.lookupCaller(ch, bob))
        coVerify(exactly = 0) { dmApi.getDms() }
    }

    @Test
    fun `caller in the DM list from the server is found`() = runTest {
        coEvery { dmApi.getDms() } returns listOf(dm(meParticipant, bobParticipant))

        assertEquals(CallerLookup.Found("Bob"), resolver.lookupCaller(ch, bob))
    }

    @Test
    fun `DM without the caller among its participants is found without a name`() = runTest {
        coEvery { dmApi.getDms() } returns listOf(dm(meParticipant))

        assertEquals(CallerLookup.Found(null), resolver.lookupCaller(ch, bob))
    }

    @Test
    fun `channel missing from the loaded DM list is not the user's`() = runTest {
        coEvery { dmApi.getDms() } returns listOf(DmConversation(id = "other", participants = listOf(meParticipant)))

        assertEquals(CallerLookup.NotMember, resolver.lookupCaller(ch, bob))
    }

    @Test
    fun `forbidden or not found answers mean not the user's`() = runTest {
        coEvery { dmApi.getDms() } throws ApiException(HttpStatusCode.Forbidden, "forbidden")
        assertEquals(CallerLookup.NotMember, resolver.lookupCaller(ch, bob))

        coEvery { dmApi.getDms() } throws ApiException(HttpStatusCode.NotFound, "not found")
        assertEquals(CallerLookup.NotMember, resolver.lookupCaller(ch, bob))
    }

    @Test
    fun `network or server errors are unknown`() = runTest {
        coEvery { dmApi.getDms() } throws IOException("offline")
        assertEquals(CallerLookup.Unknown, resolver.lookupCaller(ch, bob))

        coEvery { dmApi.getDms() } throws ApiException(HttpStatusCode.ServiceUnavailable, "unavailable")
        assertEquals(CallerLookup.Unknown, resolver.lookupCaller(ch, bob))
    }
}
