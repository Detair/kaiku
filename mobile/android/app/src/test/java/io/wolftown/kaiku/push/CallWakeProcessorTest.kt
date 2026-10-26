package io.wolftown.kaiku.push

import io.mockk.coEvery
import io.mockk.coVerify
import io.mockk.every
import io.mockk.mockk
import io.wolftown.kaiku.data.auth.RefreshResult
import io.wolftown.kaiku.data.auth.TokenRefresher
import io.wolftown.kaiku.data.local.TokenStorage
import io.wolftown.kaiku.testutil.InMemorySharedPreferences
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
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

    private var clock = 1_000_000L
    private val tokenStorage: TokenStorage = mockk()
    private val refresher: TokenRefresher = mockk()
    private val names: NotificationTitleResolver = mockk()
    private val store = PrefsCallNotificationStore(InMemorySharedPreferences()) { clock }
    private val sink = FakeCallSink()
    private val processor = CallWakeProcessor(tokenStorage, refresher, names, store, sink).also { it.now = { clock } }
    private val intake = CallWakeIntake(tokenStorage, store).also { it.now = { clock } }

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

    /** A ringing wake as the scheduler handles it: intake, then the work. */
    private suspend fun ring(callId: String = callA, isStopped: () -> Boolean = { false }): CallOutcome {
        intake.accept(ringing(callId))
        return processor.process(ringing(callId), owner, isStopped)
    }

    private fun missed(callId: String = callA, name: String? = "Bob") =
        CallNotification("me", ch, callId, CallKind.MISSED, name)

    @Test
    fun `ringing posts the incoming call with the caller name`() = runTest {
        assertEquals(CallOutcome.INCOMING, ring())

        assertEquals(CallNotification("me", ch, callA, CallKind.INCOMING, "Bob"), sink.shown[ch])
        coVerify { names.callerName(ch, bob) }
    }

    @Test
    fun `ended no_answer replaces the call with a missed call`() = runTest {
        ring()

        assertEquals(CallOutcome.MISSED, processor.process(ended(CallEndReason.NO_ANSWER), owner))

        assertEquals(missed(), sink.shown[ch])
        assertEquals(1, sink.shown.size)
        assertTrue(sink.cancelled.isEmpty())
        assertEquals(CallPhase.MISSED, store.shown("me", ch)?.phase)
    }

    @Test
    fun `ended cancelled shows a missed call`() = runTest {
        ring()

        assertEquals(CallOutcome.MISSED, processor.process(ended(CallEndReason.CANCELLED), owner))

        assertEquals(CallKind.MISSED, sink.shown[ch]?.kind)
    }

    @Test
    fun `ended all_declined removes the call and leaves a tombstone`() = runTest {
        ring()

        assertEquals(CallOutcome.CANCELLED, processor.process(ended(CallEndReason.ALL_DECLINED), owner))

        assertEquals(listOf(ch), sink.cancelled)
        assertTrue(sink.shown.isEmpty())
        assertEquals(ShownCall(callA, CallPhase.ENDED, clock), store.shown("me", ch))
    }

    @Test
    fun `ended last_left removes the call notification`() = runTest {
        ring()

        assertEquals(CallOutcome.CANCELLED, processor.process(ended(CallEndReason.LAST_LEFT), owner))

        assertEquals(listOf(ch), sink.cancelled)
        assertTrue(sink.shown.isEmpty())
        assertEquals(CallPhase.ENDED, store.shown("me", ch)?.phase)
    }

    @Test
    fun `reordered ringing after the call ended is not accepted again`() = runTest {
        ring()
        processor.process(ended(CallEndReason.ALL_DECLINED), owner)

        assertFalse(intake.accept(ringing()))
        assertEquals(CallOutcome.IGNORED, processor.process(ringing(), owner))
        assertEquals(CallOutcome.IGNORED, processor.process(ended(CallEndReason.NO_ANSWER), owner))

        assertTrue(sink.shown.isEmpty())
        assertEquals(1, sink.posts.size)
    }

    @Test
    fun `ended for another call id is ignored`() = runTest {
        ring(callA)

        assertEquals(CallOutcome.IGNORED, processor.process(ended(CallEndReason.NO_ANSWER, callB), owner))
        assertEquals(CallOutcome.IGNORED, processor.process(ended(CallEndReason.ALL_DECLINED, callB), owner))

        assertEquals(CallKind.INCOMING, sink.shown[ch]?.kind)
        assertEquals(callA, sink.shown[ch]?.callId)
        assertTrue(sink.cancelled.isEmpty())
    }

    @Test
    fun `ended no_answer with no call stored shows the missed call`() = runTest {
        // Offline during the ringing; the collapse key kept only the ended.
        assertEquals(CallOutcome.MISSED, processor.process(ended(CallEndReason.NO_ANSWER), owner))

        assertEquals(listOf(missed()), sink.posts)
        assertEquals(CallPhase.MISSED, store.shown("me", ch)?.phase)
    }

    @Test
    fun `ended cancelled with no call stored shows the missed call`() = runTest {
        assertEquals(CallOutcome.MISSED, processor.process(ended(CallEndReason.CANCELLED), owner))
        assertEquals(listOf(missed()), sink.posts)
    }

    @Test
    fun `ended all_declined with no call stored leaves only a tombstone`() = runTest {
        assertEquals(CallOutcome.CANCELLED, processor.process(ended(CallEndReason.ALL_DECLINED), owner))

        assertTrue(sink.posts.isEmpty())
        assertFalse(intake.accept(ringing()))
    }

    @Test
    fun `ended for a stored tombstone is ignored`() = runTest {
        store.setShown("me", ch, ShownCall(callA, CallPhase.ENDED, clock))

        assertEquals(CallOutcome.IGNORED, processor.process(ended(CallEndReason.NO_ANSWER), owner))

        assertTrue(sink.posts.isEmpty())
    }

    @Test
    fun `ringing with a new call id replaces the old call`() = runTest {
        ring(callA)

        assertEquals(CallOutcome.INCOMING, ring(callB))

        assertEquals(callB, sink.shown[ch]?.callId)
        assertEquals(CallOutcome.IGNORED, processor.process(ended(CallEndReason.NO_ANSWER, callA), owner))
        assertEquals(CallOutcome.MISSED, processor.process(ended(CallEndReason.NO_ANSWER, callB), owner))
        assertEquals(missed(callB), sink.shown[ch])
    }

    @Test
    fun `duplicate ended after a missed call posts nothing new`() = runTest {
        ring()
        processor.process(ended(CallEndReason.NO_ANSWER), owner)

        assertEquals(CallOutcome.IGNORED, processor.process(ended(CallEndReason.NO_ANSWER), owner))
        assertEquals(CallOutcome.IGNORED, processor.process(ringing(), owner))

        assertEquals(2, sink.posts.size)
        assertEquals(CallKind.MISSED, sink.shown[ch]?.kind)
    }

    @Test
    fun `ringing work without its accepted call posts nothing`() = runTest {
        // Only the intake makes a call current; a ringing for another stored call never posts.
        intake.accept(ringing(callB))

        assertEquals(CallOutcome.IGNORED, processor.process(ringing(callA), owner))

        assertTrue(sink.posts.isEmpty())
    }

    @Test
    fun `ringing older than the ring timeout posts nothing`() = runTest {
        intake.accept(ringing())
        clock += CallWakeProcessor.RING_MAX_AGE_MS + 1

        assertEquals(CallOutcome.IGNORED, processor.process(ringing(), owner))
        assertTrue(sink.posts.isEmpty())
        coVerify(exactly = 0) { names.callerName(any(), any()) }

        // Its end still shows the missed call.
        assertEquals(CallOutcome.MISSED, processor.process(ended(CallEndReason.NO_ANSWER), owner))
    }

    @Test
    fun `ringing just within the ring timeout still posts`() = runTest {
        intake.accept(ringing())
        clock += CallWakeProcessor.RING_MAX_AGE_MS

        assertEquals(CallOutcome.INCOMING, processor.process(ringing(), owner))
    }

    @Test
    fun `failed name lookup uses the generic text`() = runTest {
        coEvery { names.callerName(ch, bob) } returns null

        assertEquals(CallOutcome.INCOMING, ring())
        assertNull(sink.shown[ch]?.callerName)

        assertEquals(CallOutcome.MISSED, processor.process(ended(CallEndReason.NO_ANSWER), owner))
        assertEquals(missed(name = null), sink.shown[ch])
    }

    @Test
    fun `token refresh failing on the network still posts with the generic text`() = runTest {
        coEvery { refresher.ensureFresh(any()) } returns RefreshResult.NETWORK_ERROR

        assertEquals(CallOutcome.INCOMING, ring())

        assertNull(sink.shown[ch]?.callerName)
        coVerify(exactly = 0) { names.callerName(any(), any()) }
    }

    @Test
    fun `no session posts nothing`() = runTest {
        refreshToken = null

        assertEquals(CallOutcome.NOT_LOGGED_IN, processor.process(ringing(), owner))
        assertEquals(CallOutcome.NOT_LOGGED_IN, processor.process(ended(CallEndReason.NO_ANSWER), owner))

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
        assertEquals(CallOutcome.WRONG_ACCOUNT, processor.process(ended(CallEndReason.NO_ANSWER), WakeOwner("other", server)))
        assertEquals(
            CallOutcome.WRONG_ACCOUNT,
            processor.process(ended(CallEndReason.NO_ANSWER), WakeOwner("me", "https://elsewhere.example")),
        )

        assertTrue(sink.posts.isEmpty())
        assertNull(store.shown("me", ch))
        assertNull(store.shown("other", ch))
    }

    @Test
    fun `session ending during the name lookup posts nothing`() = runTest {
        intake.accept(ringing())
        coEvery { names.callerName(ch, bob) } answers {
            generation = 2L
            "Bob"
        }

        assertEquals(CallOutcome.NOT_LOGGED_IN, processor.process(ringing(), owner))

        assertTrue(sink.posts.isEmpty())
    }

    @Test
    fun `logout right before posting posts nothing`() = runTest {
        sink.beforePost = { refreshToken = null }

        assertEquals(CallOutcome.NOT_LOGGED_IN, ring())

        assertTrue(sink.posts.isEmpty())
    }

    @Test
    fun `missed call is not posted for another account`() = runTest {
        ring()
        userId = "other"

        assertEquals(CallOutcome.WRONG_ACCOUNT, processor.process(ended(CallEndReason.NO_ANSWER), owner))

        assertEquals(CallKind.INCOMING, sink.shown[ch]?.kind)
    }

    @Test
    fun `stopped work posts nothing`() = runTest {
        assertEquals(CallOutcome.IGNORED, ring { true })
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
        ring()

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
    fun `disabled notifications keep the accepted call for its end`() = runTest {
        sink.disabled = true

        assertEquals(CallOutcome.NOTIFICATIONS_OFF, ring())

        assertEquals(CallPhase.RINGING, store.shown("me", ch)?.phase)
    }

    @Test
    fun `ended for an accepted ringing whose work was replaced shows the missed call`() = runTest {
        // The ringing work never ran: the ended wake replaced it.
        intake.accept(ringing())

        assertEquals(CallOutcome.MISSED, processor.process(ended(CallEndReason.NO_ANSWER), owner))

        assertEquals(listOf(missed()), sink.posts)
    }

    @Test
    fun `ended while the ringing work resolves the name leaves no ringing behind`() = runTest {
        intake.accept(ringing())
        var stopped = false
        var nested = false
        coEvery { names.callerName(ch, bob) } coAnswers {
            if (!nested) {
                nested = true
                // The ended wake arrives and replaces the still running ringing work.
                assertEquals(CallOutcome.MISSED, processor.process(ended(CallEndReason.NO_ANSWER), owner))
                stopped = true
            }
            "Bob"
        }

        assertEquals(CallOutcome.IGNORED, processor.process(ringing(), owner) { stopped })

        assertEquals(listOf(missed()), sink.posts)
    }

    @Test
    fun `ringing still running after its missed call posts nothing even if not stopped`() = runTest {
        intake.accept(ringing())
        var nested = false
        coEvery { names.callerName(ch, bob) } coAnswers {
            if (!nested) {
                nested = true
                processor.process(ended(CallEndReason.NO_ANSWER), owner)
            }
            "Bob"
        }

        assertEquals(CallOutcome.IGNORED, processor.process(ringing(), owner))

        assertEquals(listOf(missed()), sink.posts)
    }

    @Test
    fun `accepting a newer ringing makes the older call's end ignored`() = runTest {
        intake.accept(ringing(callA))
        intake.accept(ringing(callB))

        assertEquals(CallOutcome.IGNORED, processor.process(ended(CallEndReason.NO_ANSWER, callA), owner))
        assertTrue(sink.posts.isEmpty())
    }
}
