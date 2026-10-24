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
import io.wolftown.kaiku.data.api.UnreadApi
import io.wolftown.kaiku.data.auth.RefreshResult
import io.wolftown.kaiku.data.auth.TokenRefresher
import io.wolftown.kaiku.data.local.TokenStorage
import io.wolftown.kaiku.data.repository.ChatRepository
import io.wolftown.kaiku.data.repository.DmRepository
import io.wolftown.kaiku.domain.model.ChannelUnread
import io.wolftown.kaiku.domain.model.CursorPage
import io.wolftown.kaiku.domain.model.GuildUnreadSummary
import io.wolftown.kaiku.domain.model.Message
import io.wolftown.kaiku.domain.model.UnreadAggregate
import io.wolftown.kaiku.domain.model.User
import io.wolftown.kaiku.testutil.InMemorySharedPreferences
import kotlinx.coroutines.test.runTest
import java.io.IOException
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test

class WakeFetchProcessorTest {

    private val ch = "0f8fad5b-d9cb-469f-a165-70867728950e"
    private val server = "https://kaiku.example"
    private val me = User(id = "me", username = "me", displayName = "Me")
    private val bob = User(id = "bob", username = "bob", displayName = "Bob")
    private val owner = WakeOwner("me", server)

    private val refresher: TokenRefresher = mockk()
    private val tokenStorage: TokenStorage = mockk()
    private val messages: MessageApi = mockk()
    private val unreadApi: UnreadApi = mockk()
    private val titles: NotificationTitleResolver = mockk()
    private val sink: NotificationSink = mockk(relaxed = true)
    private val chat: ChatRepository = mockk(relaxed = true)
    private val dms: DmRepository = mockk(relaxed = true)
    private val active = ActiveChannelTracker()
    private val ledger = PrefsNotificationLedger(InMemorySharedPreferences())

    private val enqueuer = FakeWakeWorkEnqueuer()
    private val processor = WakeFetchProcessor(
        refresher, tokenStorage, messages, unreadApi, titles, ledger, sink, enqueuer, active, chat, dms,
    )
    private val scheduler = WakeScheduler(enqueuer, sink)

    private val shown = mutableListOf<ChannelNotification>()

    /** Current session as TokenStorage reports it. */
    private var generation = 1L
    private var refreshToken: String? = "refresh"

    /** Run inside the presenter's lock, right before posting. */
    private var beforePost: () -> Unit = {}

    /** Run right after a notification was posted. */
    private var afterPost: () -> Unit = {}

    /** The user turned notifications off. */
    private var notificationsDisabled = false

    /** Server unread count of the woken DM channel. */
    private fun serverDmUnread(count: Long) {
        coEvery { unreadApi.getUnread() } returns UnreadAggregate(dms = listOf(ChannelUnread(ch, unreadCount = count)))
    }

    private fun msg(id: String, author: User, minute: Int) = Message(
        id = id,
        channelId = ch,
        author = author,
        content = "text $id",
        createdAt = "2026-10-24T10:%02d:00Z".format(minute),
    )

    private fun page(vararg items: Message, hasMore: Boolean = false, next: String? = null) =
        CursorPage(items = items.toList(), hasMore = hasMore, nextCursor = next)

    private fun dm(count: Int = 1) = WakeSignal(WakeKind.DM, ch, count)

    @Before
    fun setUp() {
        every { tokenStorage.getRefreshToken() } answers { refreshToken }
        every { tokenStorage.getUserId() } returns "me"
        every { tokenStorage.getServerUrl() } returns server
        every { tokenStorage.getSessionGeneration() } answers { generation }
        coEvery { refresher.ensureFresh(any()) } returns RefreshResult.SUCCESS
        coEvery { titles.resolve(any(), any()) } returns ("Bob" to true)
        serverDmUnread(100)
        val prepare = slot<() -> ChannelNotification?>()
        every { sink.show(capture(prepare)) } answers {
            beforePost()
            val n = prepare.captured()
            when {
                n == null -> ShowResult.REFUSED
                notificationsDisabled -> ShowResult.DISABLED
                else -> {
                    shown += n
                    afterPost()
                    ShowResult.POSTED
                }
            }
        }
    }

    @Test
    fun `expired token is refreshed before fetching`() = runTest {
        coEvery { messages.getMessages(ch, null, null, any()) } returns page(msg("m1", bob, 1))

        assertEquals(WakeOutcome.NOTIFIED, processor.process(dm(), owner))

        coVerifyOrder {
            refresher.ensureFresh(any())
            messages.getMessages(ch, null, null, any())
        }
        assertEquals("me", shown.single().userId)
    }

    @Test
    fun `network error on refresh returns RETRY`() = runTest {
        coEvery { refresher.ensureFresh(any()) } returns RefreshResult.NETWORK_ERROR

        assertEquals(WakeOutcome.RETRY, processor.process(dm(), owner))

        verify(exactly = 0) { sink.show(any()) }
        coVerify(exactly = 0) { messages.getMessages(any(), any(), any(), any()) }
    }

    @Test
    fun `rejected session returns NOT_LOGGED_IN and posts nothing`() = runTest {
        coEvery { refresher.ensureFresh(any()) } returns RefreshResult.AUTH_REJECTED
        assertEquals(WakeOutcome.NOT_LOGGED_IN, processor.process(dm(), owner))

        coEvery { refresher.ensureFresh(any()) } returns RefreshResult.NO_SESSION
        assertEquals(WakeOutcome.NOT_LOGGED_IN, processor.process(dm(), owner))

        verify(exactly = 0) { sink.show(any()) }
        coVerify(exactly = 0) { messages.getMessages(any(), any(), any(), any()) }
    }

    @Test
    fun `rejected session clears notifications once the session is gone`() = runTest {
        coEvery { refresher.ensureFresh(any()) } answers {
            refreshToken = null
            RefreshResult.AUTH_REJECTED
        }

        assertEquals(WakeOutcome.NOT_LOGGED_IN, processor.process(dm(), owner))

        verify { sink.cancelAll() }
        assertEquals(1, enqueuer.cancelAllCount)
    }

    @Test
    fun `no refresh token returns NOT_LOGGED_IN without refreshing`() = runTest {
        refreshToken = null

        assertEquals(WakeOutcome.NOT_LOGGED_IN, processor.process(dm(), owner))

        coVerify(exactly = 0) { refresher.ensureFresh(any()) }
    }

    @Test
    fun `wake queued for another account is dropped before refreshing`() = runTest {
        assertEquals(WakeOutcome.WRONG_ACCOUNT, processor.process(dm(), WakeOwner("alice", server)))
        assertEquals(WakeOutcome.WRONG_ACCOUNT, processor.process(dm(), WakeOwner("me", "https://other.example")))

        coVerify(exactly = 0) { refresher.ensureFresh(any()) }
        coVerify(exactly = 0) { messages.getMessages(any(), any(), any(), any()) }
    }

    @Test
    fun `session change during refresh posts nothing`() = runTest {
        // A new login during the refresh bumps the generation (a rotation would not).
        coEvery { refresher.ensureFresh(any()) } answers {
            generation = 2L
            RefreshResult.SUCCESS
        }

        assertEquals(WakeOutcome.NOT_LOGGED_IN, processor.process(dm(), owner))

        coVerify(exactly = 0) { messages.getMessages(any(), any(), any(), any()) }
    }

    @Test
    fun `wake for visible channel suppresses notification and catches up`() = runTest {
        active.onVisible(ch)

        assertEquals(WakeOutcome.SUPPRESSED_ACTIVE, processor.process(dm(), owner))

        coVerify { chat.catchUp(ch) }
        verify(exactly = 0) { sink.show(any()) }
        coVerify(exactly = 0) { messages.getMessages(any(), any(), any(), any()) }
    }

    @Test
    fun `chat opened during fetch posts nothing`() = runTest {
        coEvery { messages.getMessages(ch, null, null, any()) } answers {
            active.onVisible(ch)
            page(msg("m1", bob, 1))
        }

        assertEquals(WakeOutcome.SUPPRESSED_ACTIVE, processor.process(dm(), owner))

        assertTrue(shown.isEmpty())
        assertEquals("m1", ledger.lastSeen("me", ch))
    }

    @Test
    fun `chat opened right before posting posts nothing`() = runTest {
        coEvery { messages.getMessages(ch, null, null, any()) } returns page(msg("m1", bob, 1))
        beforePost = { active.onVisible(ch) }

        assertEquals(WakeOutcome.SUPPRESSED_ACTIVE, processor.process(dm(), owner))

        assertTrue(shown.isEmpty())
    }

    @Test
    fun `wake fetches after last seen id and notifies only others messages`() = runTest {
        ledger.markSeen("me", ch, "m1", "2026-10-24T10:01:00Z")
        coEvery { messages.getMessages(ch, null, "m1", 50) } returns page(msg("m2", bob, 2), msg("m3", me, 3))

        assertEquals(WakeOutcome.NOTIFIED, processor.process(dm(), owner))

        val n = shown.single()
        assertEquals(listOf("m2"), n.lines.map { it.messageId })
        assertEquals("Bob", n.lines.single().senderName)
        assertEquals(LineText.Raw("text m2"), n.lines.single().text)
        assertEquals("Bob", n.title)
        // The own newer message is seen too.
        assertEquals("m3", ledger.lastSeen("me", ch))
        coVerify { chat.catchUp(ch) }
    }

    @Test
    fun `second wake fetches only messages after last notified id`() = runTest {
        coEvery { messages.getMessages(ch, null, null, any()) } returns page(msg("m1", bob, 1))
        processor.process(dm(), owner)
        assertEquals("m1", ledger.lastSeen("me", ch))

        coEvery { messages.getMessages(ch, null, "m1", 50) } returns page(msg("m2", bob, 2))
        assertEquals(WakeOutcome.NOTIFIED, processor.process(dm(), owner))

        coVerify { messages.getMessages(ch, null, "m1", 50) }
        assertEquals(listOf("m2"), shown.last().lines.map { it.messageId })
        assertEquals("m2", ledger.lastSeen("me", ch))
    }

    @Test
    fun `no ledger entry fetches latest page limited by count`() = runTest {
        coEvery { messages.getMessages(ch, null, null, any()) } returns page()

        processor.process(dm(count = 40), owner)
        coVerify { messages.getMessages(ch, null, null, 10) }

        processor.process(dm(count = 0), owner)
        coVerify { messages.getMessages(ch, null, null, 1) }
    }

    @Test
    fun `unknown anchor 400 falls back to latest page`() = runTest {
        ledger.markSeen("me", ch, "m1", "2026-10-24T10:01:00Z")
        coEvery { messages.getMessages(ch, null, "m1", 50) } throws ApiException(HttpStatusCode.BadRequest, "Unknown cursor")
        // Latest page is newest first; m0 is older than the last seen position.
        coEvery { messages.getMessages(ch, null, null, any()) } returns page(msg("m2", bob, 2), msg("m0", bob, 0))

        assertEquals(WakeOutcome.NOTIFIED, processor.process(dm(count = 3), owner))

        coVerify { messages.getMessages(ch, null, null, 3) }
        assertEquals(listOf("m2"), shown.single().lines.map { it.messageId })
    }

    @Test
    fun `others messages followed by many own messages still notify`() = runTest {
        ledger.markSeen("me", ch, "m1", "2026-10-24T10:01:00Z")
        coEvery { messages.getMessages(ch, null, "m1", 50) } returns
            page(msg("o1", bob, 2), msg("o2", bob, 3), hasMore = true, next = "o2")
        val own = (10..19).map { msg("own$it", me, it) }.toTypedArray()
        coEvery { messages.getMessages(ch, null, "o2", 50) } returns page(*own)

        assertEquals(WakeOutcome.NOTIFIED, processor.process(dm(), owner))

        assertEquals(listOf("o1", "o2"), shown.single().lines.map { it.messageId })
        assertEquals("own19", ledger.lastSeen("me", ch))
    }

    @Test
    fun `notification keeps the latest seven eligible lines`() = runTest {
        val items = (2..11).map { msg("b$it", bob, it) } + msg("own", me, 12)
        coEvery { messages.getMessages(ch, null, null, any()) } returns page(*items.reversed().toTypedArray())

        processor.process(dm(count = 10), owner)

        assertEquals((5..11).map { "b$it" }, shown.single().lines.map { it.messageId })
        assertEquals("own", ledger.lastSeen("me", ch))
    }

    @Test
    fun `read recorded during fetch posts nothing`() = runTest {
        coEvery { messages.getMessages(ch, null, null, any()) } returns page(msg("m1", bob, 1))
        coEvery { titles.resolve(any(), any()) } coAnswers {
            ledger.markSeen("me", ch, "m1", "2026-10-24T10:01:00Z")
            "Bob" to true
        }

        assertEquals(WakeOutcome.NOTHING_NEW, processor.process(dm(), owner))

        assertTrue(shown.isEmpty())
    }

    @Test
    fun `only own messages yields NOTHING_NEW`() = runTest {
        coEvery { messages.getMessages(ch, null, null, any()) } returns page(msg("m1", me, 1))

        assertEquals(WakeOutcome.NOTHING_NEW, processor.process(dm(), owner))

        verify(exactly = 0) { sink.show(any()) }
        assertEquals("m1", ledger.lastSeen("me", ch))
    }

    @Test
    fun `logout during fetch posts nothing and leaves the ledger alone`() = runTest {
        coEvery { messages.getMessages(ch, null, null, any()) } answers {
            generation = 2L
            page(msg("m1", bob, 1))
        }

        assertEquals(WakeOutcome.NOT_LOGGED_IN, processor.process(dm(), owner))

        assertTrue(shown.isEmpty())
        assertNull(ledger.lastSeen("me", ch))
    }

    @Test
    fun `logout of only own messages leaves the ledger alone`() = runTest {
        coEvery { messages.getMessages(ch, null, null, any()) } answers {
            refreshToken = null
            page(msg("m1", me, 1))
        }

        assertEquals(WakeOutcome.NOT_LOGGED_IN, processor.process(dm(), owner))

        assertNull(ledger.lastSeen("me", ch))
    }

    @Test
    fun `logout right before posting posts nothing`() = runTest {
        coEvery { messages.getMessages(ch, null, null, any()) } returns page(msg("m1", bob, 1))
        beforePost = { generation = 2L }

        assertEquals(WakeOutcome.NOT_LOGGED_IN, processor.process(dm(), owner))

        assertTrue(shown.isEmpty())
        assertNull(ledger.lastSeen("me", ch))
    }

    @Test
    fun `unresolved title leaves title null and keeps dm flag from kind`() = runTest {
        coEvery { titles.resolve(any(), any()) } returns null
        coEvery { messages.getMessages(ch, null, null, any()) } returns page(msg("m1", bob, 1))

        processor.process(dm(), owner)

        assertNull(shown.single().title)
        assertEquals(true, shown.single().isDm)
    }

    @Test
    fun `lost channel access is not retried`() = runTest {
        coEvery { messages.getMessages(ch, null, null, any()) } throws ApiException(HttpStatusCode.Forbidden, "no")
        assertEquals(WakeOutcome.NOTHING_NEW, processor.process(dm(), owner))
    }

    @Test
    fun `server error returns RETRY`() = runTest {
        coEvery { messages.getMessages(ch, null, null, any()) } throws ApiException(HttpStatusCode.InternalServerError, "boom")
        assertEquals(WakeOutcome.RETRY, processor.process(dm(), owner))
        verify(exactly = 0) { sink.show(any()) }
    }

    @Test
    fun `dm wake reloads the dm list`() = runTest {
        coEvery { messages.getMessages(ch, null, null, any()) } returns page(msg("m1", bob, 1))

        processor.process(dm(), owner)

        coVerify { dms.loadDms() }
    }

    // -- Server read state (C20: wakes and reads are unordered) ----------------

    @Test
    fun `late wake after a read shows nothing when the server has no unread`() = runTest {
        scheduler.onWake(WakeSignal(WakeKind.READ, ch, 0))
        serverDmUnread(0)
        coEvery { messages.getMessages(ch, null, null, any()) } returns page(msg("m2", bob, 2), msg("m1", bob, 1))

        assertEquals(WakeOutcome.NOTHING_NEW, processor.process(dm(), owner))

        assertTrue(shown.isEmpty())
        assertEquals("m2", ledger.lastSeen("me", ch))
    }

    @Test
    fun `outage with a late read of m1 and a wake for newer m2 notifies m2`() = runTest {
        // The read of m1 and the wake for m2 arrive together after an outage, in any order.
        scheduler.onWake(WakeSignal(WakeKind.READ, ch, 0))
        serverDmUnread(1)
        coEvery { messages.getMessages(ch, null, null, any()) } returns page(msg("m2", bob, 2), msg("m1", bob, 1))

        assertEquals(WakeOutcome.NOTIFIED, processor.process(dm(count = 2), owner))

        assertEquals(listOf("m2"), shown.single().lines.map { it.messageId })
    }

    @Test
    fun `channel missing from the server unread list posts nothing`() = runTest {
        coEvery { unreadApi.getUnread() } returns UnreadAggregate()
        coEvery { messages.getMessages(ch, null, null, any()) } returns page(msg("m1", bob, 1))

        assertEquals(WakeOutcome.NOTHING_NEW, processor.process(dm(), owner))

        assertTrue(shown.isEmpty())
    }

    @Test
    fun `guild unread count includes own messages`() = runTest {
        coEvery { unreadApi.getUnread() } returns UnreadAggregate(
            guilds = listOf(GuildUnreadSummary(guildId = "g", channels = listOf(ChannelUnread(ch, unreadCount = 2)))),
        )
        coEvery { messages.getMessages(ch, null, null, any()) } returns
            page(msg("m3", me, 3), msg("m2", bob, 2), msg("m1", bob, 1))

        assertEquals(WakeOutcome.NOTIFIED, processor.process(WakeSignal(WakeKind.MENTION, ch, 3), owner))

        assertEquals(listOf("m2"), shown.single().lines.map { it.messageId })
    }

    @Test
    fun `unread lookup network error retries`() = runTest {
        coEvery { unreadApi.getUnread() } throws IOException("offline")
        coEvery { messages.getMessages(ch, null, null, any()) } returns page(msg("m1", bob, 1))

        assertEquals(WakeOutcome.RETRY, processor.process(dm(), owner))

        assertTrue(shown.isEmpty())
        assertNull(ledger.lastSeen("me", ch))
    }

    @Test
    fun `unread lookup refused posts nothing`() = runTest {
        coEvery { unreadApi.getUnread() } throws ApiException(HttpStatusCode.Forbidden, "no")
        coEvery { messages.getMessages(ch, null, null, any()) } returns page(msg("m1", bob, 1))

        assertEquals(WakeOutcome.NOTHING_NEW, processor.process(dm(), owner))

        assertTrue(shown.isEmpty())
    }

    @Test
    fun `exact remote read before a late wake shows nothing`() = runTest {
        ledger.markRead("me", ch, "m2", "2026-10-24T10:02:00Z")
        coEvery { messages.getMessages(ch, null, null, any()) } returns page(msg("m2", bob, 2), msg("m1", bob, 1))

        assertEquals(WakeOutcome.NOTHING_NEW, processor.process(dm(), owner))

        assertTrue(shown.isEmpty())
    }

    @Test
    fun `read push while the fetch is suspended posts nothing`() = runTest {
        var stopped = false
        coEvery { messages.getMessages(ch, null, null, any()) } answers {
            // The read cancels the running work (WorkManager stops it).
            scheduler.onWake(WakeSignal(WakeKind.READ, ch, 0))
            stopped = true
            page(msg("m1", bob, 1))
        }

        assertEquals(WakeOutcome.NOTHING_NEW, processor.process(dm(), owner) { stopped })

        assertTrue(shown.isEmpty())
        assertEquals(listOf(ch), enqueuer.cancelled)
    }

    @Test
    fun `read landing right before posting posts nothing`() = runTest {
        coEvery { messages.getMessages(ch, null, null, any()) } returns page(msg("m1", bob, 1))
        beforePost = { ledger.markRead("me", ch, "m1", "2026-10-24T10:01:00Z") }

        assertEquals(WakeOutcome.NOTHING_NEW, processor.process(dm(), owner))

        assertTrue(shown.isEmpty())
    }

    @Test
    fun `read landing right after posting takes the notification back`() = runTest {
        coEvery { messages.getMessages(ch, null, null, any()) } returns page(msg("m1", bob, 1))
        afterPost = { ledger.markRead("me", ch, "m1", "2026-10-24T10:01:00Z") }

        assertEquals(WakeOutcome.NOTHING_NEW, processor.process(dm(), owner))

        verify { sink.cancel(ch) }
    }

    @Test
    fun `logout right after posting takes the notification back`() = runTest {
        coEvery { messages.getMessages(ch, null, null, any()) } returns page(msg("m1", bob, 1))
        afterPost = { generation = 2L }

        assertEquals(WakeOutcome.NOT_LOGGED_IN, processor.process(dm(), owner))

        verify { sink.cancel(ch) }
        assertNull(ledger.lastSeen("me", ch))
    }

    @Test
    fun `uncached remote read is resolved by id so only newer messages notify`() = runTest {
        ledger.markSeen("me", ch, "m1", "2026-10-24T10:01:00Z")
        // Remote read of m5, whose timestamp was unknown here.
        ledger.markRead("me", ch, "m5", null)
        coEvery { messages.getMessages(ch, null, "m1", 50) } returns
            page(msg("m2", bob, 2), msg("m3", bob, 3), msg("m4", bob, 4), msg("m5", bob, 5), msg("m6", bob, 6))

        assertEquals(WakeOutcome.NOTIFIED, processor.process(dm(), owner))

        assertEquals(listOf("m6"), shown.single().lines.map { it.messageId })
        val cursor = ledger.readCursor("me", ch)
        assertEquals("m5", cursor.id)
        assertEquals("2026-10-24T10:05:00Z", cursor.at)
        assertNull(cursor.receivedId)
    }

    @Test
    fun `unresolvable read id filters nothing`() = runTest {
        ledger.markRead("me", ch, "gone", null)
        coEvery { messages.getMessages(ch, null, null, any()) } returns page(msg("m2", bob, 2))

        assertEquals(WakeOutcome.NOTIFIED, processor.process(dm(), owner))
    }

    @Test
    fun `notifications disabled still advance the watermark and catch up`() = runTest {
        notificationsDisabled = true
        coEvery { messages.getMessages(ch, null, null, any()) } returns page(msg("m1", bob, 1))

        assertEquals(WakeOutcome.NOTIFICATIONS_OFF, processor.process(dm(), owner))

        assertTrue(shown.isEmpty())
        assertEquals("m1", ledger.lastSeen("me", ch))
        coVerify { chat.catchUp(ch) }
        coVerify { dms.loadDms() }
    }

    @Test
    fun `stopped work posts nothing`() = runTest {
        coEvery { messages.getMessages(ch, null, null, any()) } returns page(msg("m1", bob, 1))

        assertEquals(WakeOutcome.NOTHING_NEW, processor.process(dm(), owner) { true })

        assertTrue(shown.isEmpty())
    }

    // -- Session handling ----------------------------------------------------

    @Test
    fun `401 with the session intact is retried and clears nothing`() = runTest {
        coEvery { messages.getMessages(ch, null, null, any()) } throws ApiException(HttpStatusCode.Unauthorized, "no")

        assertEquals(WakeOutcome.RETRY, processor.process(dm(), owner))

        verify(exactly = 0) { sink.cancelAll() }
        assertEquals(0, enqueuer.cancelAllCount)
    }

    @Test
    fun `session ended by the http client during fetch clears notifications`() = runTest {
        // A real rejection (401, or 403 PASSWORD_CHANGE_REQUIRED): the client clears the tokens first.
        coEvery { messages.getMessages(ch, null, null, any()) } answers {
            refreshToken = null
            throw ApiException(HttpStatusCode.Forbidden, "change password")
        }

        assertEquals(WakeOutcome.NOT_LOGGED_IN, processor.process(dm(), owner))

        verify { sink.cancelAll() }
        assertEquals(1, enqueuer.cancelAllCount)
    }

    @Test
    fun `session ended by the dm list reload is handled`() = runTest {
        coEvery { messages.getMessages(ch, null, null, any()) } returns page(msg("m1", bob, 1))
        coEvery { dms.loadDms() } answers {
            refreshToken = null
            throw ApiException(HttpStatusCode.Unauthorized, "rejected")
        }

        assertEquals(WakeOutcome.NOT_LOGGED_IN, processor.process(dm(), owner))

        verify { sink.cancelAll() }
        assertEquals(1, enqueuer.cancelAllCount)
    }

    @Test
    fun `newer session is left alone when the old one is refused`() = runTest {
        coEvery { messages.getMessages(ch, null, null, any()) } answers {
            generation = 2L
            throw ApiException(HttpStatusCode.Unauthorized, "old token")
        }

        assertEquals(WakeOutcome.NOT_LOGGED_IN, processor.process(dm(), owner))

        verify(exactly = 0) { sink.cancelAll() }
        assertEquals(0, enqueuer.cancelAllCount)
    }

    @Test
    fun `wake without any session clears leftovers`() = runTest {
        refreshToken = null

        assertEquals(WakeOutcome.NOT_LOGGED_IN, processor.process(dm(), owner))

        verify { sink.cancelAll() }
        assertEquals(1, enqueuer.cancelAllCount)
    }
}
