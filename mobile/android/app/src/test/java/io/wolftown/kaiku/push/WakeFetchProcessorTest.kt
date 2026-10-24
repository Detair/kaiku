package io.wolftown.kaiku.push

import io.ktor.http.HttpStatusCode
import io.mockk.coEvery
import io.mockk.coVerify
import io.mockk.coVerifyOrder
import io.mockk.every
import io.mockk.mockk
import io.mockk.slot
import io.mockk.verify
import io.wolftown.kaiku.data.api.ApiException
import io.wolftown.kaiku.data.api.MessageApi
import io.wolftown.kaiku.data.auth.RefreshResult
import io.wolftown.kaiku.data.auth.TokenRefresher
import io.wolftown.kaiku.data.local.TokenStorage
import io.wolftown.kaiku.data.repository.ChatRepository
import io.wolftown.kaiku.data.repository.DmRepository
import io.wolftown.kaiku.domain.model.CursorPage
import io.wolftown.kaiku.domain.model.Message
import io.wolftown.kaiku.domain.model.User
import io.wolftown.kaiku.testutil.InMemorySharedPreferences
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Before
import org.junit.Test

class WakeFetchProcessorTest {

    private val ch = "0f8fad5b-d9cb-469f-a165-70867728950e"
    private val me = User(id = "me", username = "me", displayName = "Me")
    private val bob = User(id = "bob", username = "bob", displayName = "Bob")

    private val refresher: TokenRefresher = mockk()
    private val tokenStorage: TokenStorage = mockk()
    private val messages: MessageApi = mockk()
    private val titles: NotificationTitleResolver = mockk()
    private val sink: NotificationSink = mockk(relaxed = true)
    private val chat: ChatRepository = mockk(relaxed = true)
    private val dms: DmRepository = mockk(relaxed = true)
    private val active = ActiveChannelTracker()
    private val ledger = PrefsNotificationLedger(InMemorySharedPreferences()) { "me" }

    private val processor = WakeFetchProcessor(
        refresher, tokenStorage, messages, titles, ledger, sink, active, chat, dms,
    )

    private val shown = mutableListOf<ChannelNotification>()

    private fun msg(id: String, author: User, minute: Int) = Message(
        id = id,
        channelId = ch,
        author = author,
        content = "text $id",
        createdAt = "2026-10-24T10:%02d:00Z".format(minute),
    )

    private fun page(vararg items: Message, hasMore: Boolean = false) =
        CursorPage(items = items.toList(), hasMore = hasMore)

    private fun dm(count: Int = 1) = WakeSignal(WakeKind.DM, ch, count)

    @Before
    fun setUp() {
        every { tokenStorage.getRefreshToken() } returns "refresh"
        every { tokenStorage.getUserId() } returns "me"
        every { tokenStorage.getSessionGeneration() } returns 1L
        coEvery { refresher.ensureFresh(any()) } returns RefreshResult.SUCCESS
        coEvery { titles.resolve(any(), any()) } returns ("Bob" to true)
        val captured = slot<ChannelNotification>()
        every { sink.show(capture(captured)) } answers { shown += captured.captured }
    }

    @Test
    fun `expired token is refreshed before fetching`() = runTest {
        coEvery { messages.getMessages(ch, null, null, any()) } returns page(msg("m1", bob, 1))

        assertEquals(WakeOutcome.NOTIFIED, processor.process(dm()))

        coVerifyOrder {
            refresher.ensureFresh(any())
            messages.getMessages(ch, null, null, any())
        }
    }

    @Test
    fun `network error on refresh returns RETRY`() = runTest {
        coEvery { refresher.ensureFresh(any()) } returns RefreshResult.NETWORK_ERROR

        assertEquals(WakeOutcome.RETRY, processor.process(dm()))

        verify(exactly = 0) { sink.show(any()) }
        coVerify(exactly = 0) { messages.getMessages(any(), any(), any(), any()) }
    }

    @Test
    fun `rejected session returns NOT_LOGGED_IN and posts nothing`() = runTest {
        coEvery { refresher.ensureFresh(any()) } returns RefreshResult.AUTH_REJECTED
        assertEquals(WakeOutcome.NOT_LOGGED_IN, processor.process(dm()))

        coEvery { refresher.ensureFresh(any()) } returns RefreshResult.NO_SESSION
        assertEquals(WakeOutcome.NOT_LOGGED_IN, processor.process(dm()))

        verify(exactly = 0) { sink.show(any()) }
        coVerify(exactly = 0) { messages.getMessages(any(), any(), any(), any()) }
    }

    @Test
    fun `no refresh token returns NOT_LOGGED_IN without refreshing`() = runTest {
        every { tokenStorage.getRefreshToken() } returns null

        assertEquals(WakeOutcome.NOT_LOGGED_IN, processor.process(dm()))

        coVerify(exactly = 0) { refresher.ensureFresh(any()) }
    }

    @Test
    fun `wake for visible channel suppresses notification and catches up`() = runTest {
        active.onVisible(ch)

        assertEquals(WakeOutcome.SUPPRESSED_ACTIVE, processor.process(dm()))

        coVerify { chat.catchUp(ch) }
        verify(exactly = 0) { sink.show(any()) }
        coVerify(exactly = 0) { messages.getMessages(any(), any(), any(), any()) }
    }

    @Test
    fun `wake fetches after last seen id and notifies only others messages`() = runTest {
        ledger.markSeen(ch, "m1", "2026-10-24T10:01:00Z")
        coEvery { messages.getMessages(ch, null, "m1", 50) } returns page(msg("m2", bob, 2), msg("m3", me, 3))

        assertEquals(WakeOutcome.NOTIFIED, processor.process(dm()))

        val n = shown.single()
        assertEquals(listOf("m2"), n.lines.map { it.messageId })
        assertEquals("Bob", n.lines.single().senderName)
        assertEquals(LineText.Raw("text m2"), n.lines.single().text)
        assertEquals("Bob", n.title)
        // The own newer message is seen too.
        assertEquals("m3", ledger.lastSeen(ch))
        coVerify { chat.catchUp(ch) }
    }

    @Test
    fun `second wake fetches only messages after last notified id`() = runTest {
        coEvery { messages.getMessages(ch, null, null, any()) } returns page(msg("m1", bob, 1))
        processor.process(dm())
        assertEquals("m1", ledger.lastSeen(ch))

        coEvery { messages.getMessages(ch, null, "m1", 50) } returns page(msg("m2", bob, 2))
        assertEquals(WakeOutcome.NOTIFIED, processor.process(dm()))

        coVerify { messages.getMessages(ch, null, "m1", 50) }
        assertEquals(listOf("m2"), shown.last().lines.map { it.messageId })
        assertEquals("m2", ledger.lastSeen(ch))
    }

    @Test
    fun `no ledger entry fetches latest page limited by count`() = runTest {
        coEvery { messages.getMessages(ch, null, null, any()) } returns page()

        processor.process(dm(count = 40))
        coVerify { messages.getMessages(ch, null, null, 10) }

        processor.process(dm(count = 0))
        coVerify { messages.getMessages(ch, null, null, 1) }
    }

    @Test
    fun `unknown anchor 400 falls back to latest page`() = runTest {
        ledger.markSeen(ch, "m1", "2026-10-24T10:01:00Z")
        coEvery { messages.getMessages(ch, null, "m1", 50) } throws ApiException(HttpStatusCode.BadRequest, "Unknown cursor")
        // Latest page is newest first; m0 is older than the last seen position.
        coEvery { messages.getMessages(ch, null, null, any()) } returns page(msg("m2", bob, 2), msg("m0", bob, 0))

        assertEquals(WakeOutcome.NOTIFIED, processor.process(dm(count = 3)))

        coVerify { messages.getMessages(ch, null, null, 3) }
        assertEquals(listOf("m2"), shown.single().lines.map { it.messageId })
    }

    @Test
    fun `more than one page of news shows the latest page`() = runTest {
        ledger.markSeen(ch, "m1", "2026-10-24T10:01:00Z")
        coEvery { messages.getMessages(ch, null, "m1", 50) } returns page(msg("m2", bob, 2), hasMore = true)
        coEvery { messages.getMessages(ch, null, null, 10) } returns page(msg("m9", bob, 9), msg("m8", bob, 8))

        assertEquals(WakeOutcome.NOTIFIED, processor.process(dm()))

        assertEquals(listOf("m8", "m9"), shown.single().lines.map { it.messageId })
        assertEquals("m9", ledger.lastSeen(ch))
    }

    @Test
    fun `only own messages yields NOTHING_NEW`() = runTest {
        coEvery { messages.getMessages(ch, null, null, any()) } returns page(msg("m1", me, 1))

        assertEquals(WakeOutcome.NOTHING_NEW, processor.process(dm()))

        verify(exactly = 0) { sink.show(any()) }
        assertEquals("m1", ledger.lastSeen(ch))
    }

    @Test
    fun `logout during fetch posts nothing`() = runTest {
        every { tokenStorage.getSessionGeneration() } returnsMany listOf(1L, 2L)
        coEvery { messages.getMessages(ch, null, null, any()) } returns page(msg("m1", bob, 1))

        assertEquals(WakeOutcome.NOT_LOGGED_IN, processor.process(dm()))

        verify(exactly = 0) { sink.show(any()) }
        assertNull(ledger.lastSeen(ch))
    }

    @Test
    fun `unresolved title leaves title null and keeps dm flag from kind`() = runTest {
        coEvery { titles.resolve(any(), any()) } returns null
        coEvery { messages.getMessages(ch, null, null, any()) } returns page(msg("m1", bob, 1))

        processor.process(dm())

        assertNull(shown.single().title)
        assertEquals(true, shown.single().isDm)
    }

    @Test
    fun `lost channel access is not retried`() = runTest {
        coEvery { messages.getMessages(ch, null, null, any()) } throws ApiException(HttpStatusCode.Forbidden, "no")
        assertEquals(WakeOutcome.NOTHING_NEW, processor.process(dm()))
    }

    @Test
    fun `server error returns RETRY`() = runTest {
        coEvery { messages.getMessages(ch, null, null, any()) } throws ApiException(HttpStatusCode.InternalServerError, "boom")
        assertEquals(WakeOutcome.RETRY, processor.process(dm()))
        verify(exactly = 0) { sink.show(any()) }
    }

    @Test
    fun `dm wake reloads the dm list`() = runTest {
        coEvery { messages.getMessages(ch, null, null, any()) } returns page(msg("m1", bob, 1))

        processor.process(dm())

        coVerify { dms.loadDms() }
    }
}
