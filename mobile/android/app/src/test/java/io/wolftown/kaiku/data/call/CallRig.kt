package io.wolftown.kaiku.data.call

import io.mockk.coEvery
import io.mockk.every
import io.mockk.mockk
import io.wolftown.kaiku.data.api.CallApi
import io.wolftown.kaiku.data.api.CallStateDto
import io.wolftown.kaiku.data.repository.VoiceRepository
import io.wolftown.kaiku.data.session.FakeSessions
import io.wolftown.kaiku.data.session.SessionGeneration
import io.wolftown.kaiku.data.voice.RejoinGate
import io.wolftown.kaiku.data.ws.KaikuWebSocket
import io.wolftown.kaiku.data.ws.ServerEvent
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.currentTime
import kotlinx.coroutines.test.runCurrent

/** The wall clock is somewhere else than the elapsed clock. */
internal const val WALL = 1_700_000_000_000L

internal class FakeCallApi : CallApi {
    /** The requests in the order they went out; the rig also notes the microphone service start here ("hold"). */
    val calls = mutableListOf<String>()

    /** The requests alone. */
    val requests get() = calls.filter { it != "hold" }

    /** The signed-in session each request was built for. */
    val sessions = mutableListOf<Long>()
    var current: CallStateDto? = null
    var startError: Exception? = null
    var getError: Exception? = null
    var getGate: kotlinx.coroutines.CompletableDeferred<Unit>? = null
    var declineGate: kotlinx.coroutines.CompletableDeferred<Unit>? = null
    var startGate: kotlinx.coroutines.CompletableDeferred<Unit>? = null

    /** The call id the server answers a join with. */
    var joinAnswers = "c-1"

    /** While true, every request waits for [release] before it is answered. */
    var hold = false
    private val waiting = ArrayDeque<kotlinx.coroutines.CompletableDeferred<Unit>>()

    /** The oldest request that is out gets its answer. */
    fun release() {
        waiting.removeFirstOrNull()?.complete(Unit)
    }

    fun releaseAll() {
        while (waiting.isNotEmpty()) release()
    }

    private fun sent(what: String, session: Long) {
        calls += what
        sessions += session
    }

    /** Requests that were out and whose caller was cancelled before the answer came. */
    val cancelled = mutableListOf<String>()

    /** The request is out; its answer comes when the test lets it. */
    private suspend fun answer() {
        if (!hold) return
        val what = calls.last()
        val mine = kotlinx.coroutines.CompletableDeferred<Unit>()
        waiting += mine
        try {
            mine.await()
        } catch (e: kotlinx.coroutines.CancellationException) {
            cancelled += what
            throw e
        }
    }

    override suspend fun get(channelId: String, session: Long): CallStateDto? {
        sent("get", session)
        answer()
        getGate?.await()
        getError?.let { throw it }
        return current
    }

    override suspend fun start(channelId: String, session: Long): CallStateDto {
        sent("start", session)
        answer()
        startGate?.await()
        startError?.let { throw it }
        return CallStateDto(channelId, "c-1", "ringing", startedBy = "me")
    }

    override suspend fun join(channelId: String, session: Long): CallStateDto {
        sent("join", session)
        answer()
        return CallStateDto(channelId, joinAnswers, "active", participants = listOf("anna", "me"))
    }

    override suspend fun decline(channelId: String, session: Long): CallStateDto {
        declineGate?.await()
        sent("decline", session)
        answer()
        return CallStateDto(channelId, "c-1", "ended")
    }

    override suspend fun leave(channelId: String, session: Long): CallStateDto {
        sent("leave", session)
        answer()
        return CallStateDto(channelId, "c-1", "ended")
    }
}

internal class FakeCallNotifier : CallNotifier {
    val log = mutableListOf<String>()

    /** Like the real notifier: nothing is posted for a session that is over; it shows here if tried. */
    private fun post(session: SessionGeneration.Ticket, what: String) {
        log += if (session.alive) what else "OVER:$what"
    }

    override fun ring(session: SessionGeneration.Ticket, channelId: String, callId: String, callerId: String, callerName: String?) =
        post(session, "ring:$callId:$callerName")
    override fun stopRing(session: SessionGeneration.Ticket, channelId: String, callId: String) = post(session, "stop:$callId")
    override fun silenceRing(session: SessionGeneration.Ticket, channelId: String, callId: String) = post(session, "quiet:$callId")
    override fun missed(session: SessionGeneration.Ticket, channelId: String, callId: String, callerName: String?) =
        post(session, "missed:$callId")
    override fun missedIfRinging(session: SessionGeneration.Ticket, channelId: String, callId: String, callerName: String?) =
        post(session, "missed-if-ringing:$callId")
    override fun stopAll() { log += "stopAll" }
}

/**
 * The call owner on virtual time: socket events in, REST and voice out, with a
 * signed-in session the test can end and replace.
 */
@OptIn(ExperimentalCoroutinesApi::class)
internal class CallRig(
    val test: TestScope,
    val dm: String = "dm-1",
    /** False: a process a push woke up, whose session nobody restored (the stored account counts). */
    restored: Boolean = true,
) {
    val api = FakeCallApi()
    val notifier = FakeCallNotifier()
    val events = MutableSharedFlow<ServerEvent>(extraBufferCapacity = 16)
    val callEnds = MutableSharedFlow<CallVoiceEnd>(extraBufferCapacity = 8)
    val voiceChannel = MutableStateFlow<String?>(null)
    var gate: RejoinGate? = null

    /** The tag of the voice session asked for last. */
    var tag = 0L

    /** Clock jump on top of virtual time (device sleep). */
    var slept = 0L

    /** Whether Android lets the microphone service start. */
    var holdGranted = true

    /** What the voice layer calls if the service reports later that it could not go to the foreground. */
    var serviceFailed: (() -> Unit)? = null

    /** What the voice layer tells the call while the microphone service is held for it. */
    var held: VoiceRepository.CallHold? = null

    /** The session each microphone service was asked for. */
    val holdTickets = mutableListOf<SessionGeneration.Ticket>()

    /** Everything the call layer asked of the voice layer, in order. */
    val voiceOps = mutableListOf<String>()

    /** The call's voice session as asked for last, and the mute it was opened with. */
    var session: VoiceRepository.CallSession? = null
    var openedMuted: Boolean? = null

    val voice: VoiceRepository = mockk(relaxed = true) {
        every { callEnds } returns this@CallRig.callEnds
        every { currentChannelId } returns voiceChannel
        coEvery { joinCall(any(), any<VoiceRepository.CallSession>()) } coAnswers {
            val call = secondArg<VoiceRepository.CallSession>()
            gate = call.gate
            tag = call.tag
            session = call
            voiceOps += "join"
            // Like the voice layer: the session opens with the call's mute.
            openedMuted = call.muted()
            voiceChannel.value = firstArg()
        }
        // The microphone service, asked for at the tap; in the order of the requests.
        every { holdForCall(any(), any(), any(), any(), any()) } answers {
            api.calls += "hold"
            voiceOps += "hold"
            holdTickets += firstArg<SessionGeneration.Ticket>()
            held = arg(4)
            serviceFailed = held!!.onStartFailed
            // Like the voice layer: nothing is started for a session that is over.
            holdGranted && firstArg<SessionGeneration.Ticket>().alive
        }
        every { setCallMuted(any(), any()) } answers { voiceOps += "mute" }
        coEvery { leaveChannel() } coAnswers { voiceChannel.value = null }
        // Like the voice layer: only the call's own session is left.
        coEvery { leaveCall(any()) } coAnswers {
            voiceOps += "leave"
            if (voiceChannel.value == dm) voiceChannel.value = null
        }
    }
    val socket: KaikuWebSocket = mockk(relaxed = true) { every { events } returns this@CallRig.events }
    val sessions = if (restored) FakeSessions("me") else FakeSessions(user = null).also { it.stored = "me" }

    val controller = CallController(
        api = api,
        voice = voice,
        webSocket = socket,
        sessions = sessions.generation,
        notifier = notifier,
        directory = object : CallDirectory {
            override fun nameOf(channelId: String, userId: String) = if (userId == "anna") "Anna" else null
            override fun titleOf(channelId: String) = "Anna"
        },
        clock = { test.currentTime + slept },
        voiceScope = test.backgroundScope,
        wallClock = { WALL + test.currentTime + slept },
    )

    init {
        test.runCurrent()
        controller.start()
        // The socket is this account's: its Ready says so.
        events.tryEmit(ServerEvent.Ready("me"))
        test.runCurrent()
    }

    /** The shown call, as a screen names it in its actions. */
    fun shown(): CallRef = controller.shown().ref()!!

    fun startCall(account: String = "me") = controller.submit(CallInput.Start(account, dm))
    fun accept(callId: String? = null) = controller.submit(CallInput.Accept(callId?.let { CallRef("me", dm, it) } ?: shown()))
    fun decline() = controller.submit(CallInput.Decline(shown()))
    fun setMuted(muted: Boolean) = controller.submit(CallInput.SetMuted(shown(), muted))
    fun hangUp() = controller.submit(CallInput.HangUp(controller.shown().ref() ?: CallRef("me", dm, null)))
    fun dismissEnded() = controller.submit(CallInput.DismissEnded("me", dm))
    fun ringNotification(callId: String = "c-1", account: String = "me", receivedAgoMs: Long? = null) =
        controller.submit(
            CallInput.RingNotification(CallRef(account, dm, callId), "anna", "Anna", receivedAgoMs?.let { wallNow() - it })
        )

    private fun wallNow() = WALL + test.currentTime + slept

    /** A `ringing` push wake for [callId] in [channel], received [receivedAgoMs] ago. */
    suspend fun pushRing(callId: String = "c-1", channel: String = dm, receivedAgoMs: Long = 0L, account: String = "me") =
        controller.fromPush(CallInput.PushRing(CallRef(account, channel, callId), "anna", "Anna", wallNow() - receivedAgoMs))

    /** An `ended` push wake. */
    suspend fun pushEnded(callId: String = "c-1", reason: String = "no_answer", channel: String = dm) =
        controller.fromPush(CallInput.PushEnded(CallRef("me", channel, callId), "anna", "Anna", reason))

    /** The ring notification of [callId] timed out. */
    fun ringExpired(callId: String = "c-1") =
        controller.submit(CallInput.RingExpired(CallRef("me", dm, callId), "Anna"))

    /** A new socket of [account] is authenticated. */
    fun socketOf(account: String) = server(ServerEvent.Ready(account))
    suspend fun declineFromNotification(callId: String, account: String = "me") =
        controller.submitAndAwait(CallInput.Decline(CallRef(account, dm, callId)))

    /** The stored session is removed, as the token store does it: the hook (advance, then the calls' part). */
    fun signOut() {
        sessions.generation.advance()
        controller.endForSignOut()
        sessions.session = io.wolftown.kaiku.data.local.AuthSession.LoggedOut
    }

    /** The server's call in this conversation, for `GET …/call`. */
    fun serverRings(callId: String = "c-1") {
        api.current = CallStateDto(dm, callId, "ringing", startedBy = "anna")
    }

    /** The shown call without its voice id (one call at a time; `tag` names its voice session). */
    val state: CallState
        get() = when (val s = controller.state.value) {
            is CallState.Outgoing -> s.copy(voiceId = 0L)
            is CallState.Connecting -> s.copy(voiceId = 0L)
            is CallState.Connected -> s.copy(voiceId = 0L)
            else -> s
        }

    fun server(event: ServerEvent) {
        events.tryEmit(event)
        test.runCurrent()
    }

    fun ring(callId: String = "c-1") {
        api.joinAnswers = callId
        server(ServerEvent.IncomingCall(dm, callId, "anna", "anna"))
    }
}
