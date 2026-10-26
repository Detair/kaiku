package io.wolftown.kaiku.push

import io.mockk.coEvery
import io.mockk.coVerify
import io.mockk.every
import io.mockk.mockk
import io.mockk.verify
import io.wolftown.kaiku.data.auth.RefreshResult
import io.wolftown.kaiku.data.auth.TokenRefresher
import io.wolftown.kaiku.data.local.TokenStorage
import io.wolftown.kaiku.testutil.InMemorySharedPreferences
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test

class CallWakeProcessorTest {

    private val ch = "0f8fad5b-d9cb-469f-a165-70867728950e"
    private val callA = "6f1c2a3b-4d5e-4f60-8a7b-9c0d1e2f3a4b"
    private val callB = "1b2c3d4e-5f60-4a7b-8c9d-0e1f2a3b4c5d"
    private val bob = "7a8b9c0d-1e2f-4a3b-8c4d-5e6f7a8b9c0d"
    private val server = "https://kaiku.example"
    private val owner = WakeOwner("me", server)

    private val tokenStorage: TokenStorage = mockk()
    private val refresher: TokenRefresher = mockk()
    private val names: NotificationTitleResolver = mockk()
    private val store = PrefsCallNotificationStore(InMemorySharedPreferences())
    private val sink = FakeCallSink()
    private val processor = CallWakeProcessor(tokenStorage, refresher, names, store, sink)

    private var generation = 1L
    private var refreshToken: String? = "refresh"
    private var userId: String? = "me"

    /** Records call notifications per channel the way the system shows them (same key = replaced). */
    class FakeCallSink : CallNotificationSink {
        val shown = linkedMapOf<String, CallNotification>()
        val posts = mutableListOf<CallNotification>()
        val cancelled = mutableListOf<String>()
        var disabled = false
        var beforePost: () -> Unit = {}

        override fun showCall(prepare: () -> CallNotification?): ShowResult {
            beforePost()
            val n = prepare() ?: return ShowResult.REFUSED
            if (disabled) return ShowResult.DISABLED
            shown[n.channelId] = n
            posts += n
            return ShowResult.POSTED
        }

        override fun cancelCall(channelId: String) {
            cancelled += channelId
            shown.remove(channelId)
        }
    }

    @Before
    fun setUp() {
        every { tokenStorage.getRefreshToken() } answers { refreshToken }
        every { tokenStorage.getUserId() } answers { userId }
        every { tokenStorage.getServerUrl() } returns server
        every { tokenStorage.getSessionGeneration() } answers { generation }
        coEvery { refresher.ensureFresh(any()) } returns RefreshResult.SUCCESS
        coEvery { names.callerName(ch, bob) } returns "Bob"
    }

    private fun ringing(callId: String = callA) =
        WakeSignal(WakeKind.CALL, ch, 1, CallInfo(callId, bob, CallState.RINGING))

    private fun ended(reason: CallEndReason?, callId: String = callA) =
        WakeSignal(WakeKind.CALL, ch, 1, CallInfo(callId, bob, CallState.ENDED, reason))

    @Test
    fun `ringing posts the incoming call with the caller name`() = runTest {
        assertEquals(CallOutcome.INCOMING, processor.process(ringing(), owner))

        assertEquals(CallNotification("me", ch, callA, CallKind.INCOMING, "Bob"), sink.shown[ch])
        coVerify { names.callerName(ch, bob) }
    }

    @Test
    fun `ended no_answer replaces the call with a missed call`() = runTest {
        processor.process(ringing(), owner)

        assertEquals(CallOutcome.MISSED, processor.process(ended(CallEndReason.NO_ANSWER), owner))

        assertEquals(CallNotification("me", ch, callA, CallKind.MISSED, "Bob"), sink.shown[ch])
        assertEquals(1, sink.shown.size)
        assertTrue(sink.cancelled.isEmpty())
    }

    @Test
    fun `ended cancelled shows a missed call`() = runTest {
        processor.process(ringing(), owner)

        assertEquals(CallOutcome.MISSED, processor.process(ended(CallEndReason.CANCELLED), owner))

        assertEquals(CallKind.MISSED, sink.shown[ch]?.kind)
    }

    @Test
    fun `ended all_declined removes the call notification`() = runTest {
        processor.process(ringing(), owner)

        assertEquals(CallOutcome.CANCELLED, processor.process(ended(CallEndReason.ALL_DECLINED), owner))

        assertEquals(listOf(ch), sink.cancelled)
        assertTrue(sink.shown.isEmpty())
        assertNull(store.shown("me", ch))
    }

    @Test
    fun `ended last_left removes the call notification`() = runTest {
        processor.process(ringing(), owner)

        assertEquals(CallOutcome.CANCELLED, processor.process(ended(CallEndReason.LAST_LEFT), owner))

        assertEquals(listOf(ch), sink.cancelled)
        assertTrue(sink.shown.isEmpty())
    }

    @Test
    fun `ended for another call id is ignored`() = runTest {
        processor.process(ringing(callA), owner)

        assertEquals(CallOutcome.IGNORED, processor.process(ended(CallEndReason.NO_ANSWER, callB), owner))
        assertEquals(CallOutcome.IGNORED, processor.process(ended(CallEndReason.ALL_DECLINED, callB), owner))

        assertEquals(CallKind.INCOMING, sink.shown[ch]?.kind)
        assertEquals(callA, sink.shown[ch]?.callId)
        assertTrue(sink.cancelled.isEmpty())
    }

    @Test
    fun `ended without a shown call is ignored`() = runTest {
        assertEquals(CallOutcome.IGNORED, processor.process(ended(CallEndReason.NO_ANSWER), owner))
        assertTrue(sink.posts.isEmpty())
        assertTrue(sink.cancelled.isEmpty())
    }

    @Test
    fun `ringing with a new call id replaces the old call`() = runTest {
        processor.process(ringing(callA), owner)

        assertEquals(CallOutcome.INCOMING, processor.process(ringing(callB), owner))

        assertEquals(callB, sink.shown[ch]?.callId)
        assertEquals(CallOutcome.IGNORED, processor.process(ended(CallEndReason.NO_ANSWER, callA), owner))
        assertEquals(CallOutcome.MISSED, processor.process(ended(CallEndReason.NO_ANSWER, callB), owner))
        assertEquals(CallNotification("me", ch, callB, CallKind.MISSED, "Bob"), sink.shown[ch])
    }

    @Test
    fun `duplicate ended after a missed call posts nothing new`() = runTest {
        processor.process(ringing(), owner)
        processor.process(ended(CallEndReason.NO_ANSWER), owner)

        assertEquals(CallOutcome.IGNORED, processor.process(ended(CallEndReason.NO_ANSWER), owner))
        assertEquals(CallOutcome.IGNORED, processor.process(ringing(), owner))

        assertEquals(2, sink.posts.size)
        assertEquals(CallKind.MISSED, sink.shown[ch]?.kind)
    }

    @Test
    fun `failed name lookup uses the generic text`() = runTest {
        coEvery { names.callerName(ch, bob) } returns null

        assertEquals(CallOutcome.INCOMING, processor.process(ringing(), owner))
        assertNull(sink.shown[ch]?.callerName)

        assertEquals(CallOutcome.MISSED, processor.process(ended(CallEndReason.NO_ANSWER), owner))
        assertEquals(CallNotification("me", ch, callA, CallKind.MISSED, null), sink.shown[ch])
    }

    @Test
    fun `token refresh failing on the network still posts with the generic text`() = runTest {
        coEvery { refresher.ensureFresh(any()) } returns RefreshResult.NETWORK_ERROR

        assertEquals(CallOutcome.INCOMING, processor.process(ringing(), owner))

        assertNull(sink.shown[ch]?.callerName)
        coVerify(exactly = 0) { names.callerName(any(), any()) }
    }

    @Test
    fun `no session posts nothing`() = runTest {
        refreshToken = null

        assertEquals(CallOutcome.NOT_LOGGED_IN, processor.process(ringing(), owner))

        assertTrue(sink.posts.isEmpty())
        assertNull(store.shown("me", ch))
    }

    @Test
    fun `signed out user posts nothing`() = runTest {
        userId = null

        assertEquals(CallOutcome.NOT_LOGGED_IN, processor.process(ringing(), owner))

        assertTrue(sink.posts.isEmpty())
    }

    @Test
    fun `wake for another account posts nothing`() = runTest {
        assertEquals(CallOutcome.WRONG_ACCOUNT, processor.process(ringing(), WakeOwner("other", server)))
        assertEquals(CallOutcome.WRONG_ACCOUNT, processor.process(ringing(), WakeOwner("me", "https://elsewhere.example")))

        assertTrue(sink.posts.isEmpty())
        assertNull(store.shown("me", ch))
        assertNull(store.shown("other", ch))
    }

    @Test
    fun `session ending during the name lookup posts nothing`() = runTest {
        coEvery { names.callerName(ch, bob) } answers {
            generation = 2L
            "Bob"
        }

        assertEquals(CallOutcome.NOT_LOGGED_IN, processor.process(ringing(), owner))

        assertTrue(sink.posts.isEmpty())
        assertNull(store.shown("me", ch))
    }

    @Test
    fun `logout right before posting posts nothing`() = runTest {
        sink.beforePost = { refreshToken = null }

        assertEquals(CallOutcome.NOT_LOGGED_IN, processor.process(ringing(), owner))

        assertTrue(sink.posts.isEmpty())
    }

    @Test
    fun `missed call is not posted for another account`() = runTest {
        processor.process(ringing(), owner)
        userId = "other"

        assertEquals(CallOutcome.WRONG_ACCOUNT, processor.process(ended(CallEndReason.NO_ANSWER), owner))

        assertEquals(CallKind.INCOMING, sink.shown[ch]?.kind)
    }

    @Test
    fun `stopped work posts nothing`() = runTest {
        assertEquals(CallOutcome.IGNORED, processor.process(ringing(), owner) { true })
        assertTrue(sink.posts.isEmpty())
    }

    @Test
    fun `unknown call state is ignored`() = runTest {
        val signal = WakeSignal(WakeKind.CALL, ch, 1, CallInfo(callA, bob, CallState.UNKNOWN))
        assertEquals(CallOutcome.IGNORED, processor.process(signal, owner))
        assertTrue(sink.posts.isEmpty())
    }

    @Test
    fun `ended with unknown or missing reason is ignored`() = runTest {
        processor.process(ringing(), owner)

        assertEquals(CallOutcome.IGNORED, processor.process(ended(CallEndReason.UNKNOWN), owner))
        assertEquals(CallOutcome.IGNORED, processor.process(ended(null), owner))

        assertEquals(CallKind.INCOMING, sink.shown[ch]?.kind)
        assertTrue(sink.cancelled.isEmpty())
    }

    @Test
    fun `signal without call info is ignored`() = runTest {
        assertEquals(CallOutcome.IGNORED, processor.process(WakeSignal(WakeKind.CALL, ch, 1), owner))
        assertTrue(sink.posts.isEmpty())
    }

    @Test
    fun `disabled notifications still record the shown call`() = runTest {
        sink.disabled = true

        assertEquals(CallOutcome.NOTIFICATIONS_OFF, processor.process(ringing(), owner))

        assertEquals(callA, store.shown("me", ch)?.callId)
    }

    @Test
    fun `read wake does not cancel the missed call notification`() = runTest {
        processor.process(ringing(), owner)
        processor.process(ended(CallEndReason.NO_ANSWER), owner)
        val messageSink: NotificationSink = mockk(relaxed = true)
        val scheduler = WakeScheduler(FakeWakeWorkEnqueuer(), messageSink)

        scheduler.onWake(WakeSignal(WakeKind.READ, ch, 0))

        verify { messageSink.cancel(ch) }
        assertTrue(sink.cancelled.isEmpty())
        assertEquals(CallKind.MISSED, sink.shown[ch]?.kind)
        assertEquals(callA, store.shown("me", ch)?.callId)
    }

    @Test
    fun `call notifications use an id apart from message notifications`() {
        assertNotEquals(AndroidNotificationPresenter.MESSAGE_ID, AndroidNotificationPresenter.CALL_ID)
        assertNotEquals(AndroidNotificationPresenter.SUMMARY_ID, AndroidNotificationPresenter.CALL_ID)
    }
}
