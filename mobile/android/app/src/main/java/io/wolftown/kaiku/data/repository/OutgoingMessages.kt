package io.wolftown.kaiku.data.repository

import android.net.Uri
import io.wolftown.kaiku.data.api.SessionChangedException
import io.wolftown.kaiku.data.local.AuthSession
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.di.ChatCoroutineScope
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.Job
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.isActive
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import java.util.UUID
import java.util.concurrent.ConcurrentHashMap
import java.util.logging.Level
import java.util.logging.Logger
import javax.inject.Inject
import javax.inject.Singleton
import kotlin.coroutines.cancellation.CancellationException

/** A message the user sent that the server has not confirmed yet (AND-B-3). */
data class PendingMessage(
    /** Local id; never sent to the server. */
    val localId: String,
    val channelId: String,
    /** The text, or the caption of [attachment] (may be empty then). */
    val text: String,
    /** The picked photo or video, or null for a text message. */
    val attachment: Uri? = null,
    val state: State = State.SENDING,
    /** Why the last attempt failed; null while [State.SENDING]. */
    val failure: Failure? = null,
    /**
     * The sign-in session it was sent in ([AuthSession.LoggedIn.sessionId]).
     * It is only ever sent in that session; when the session ends it is dropped.
     */
    val sessionId: Long? = null,
) {
    enum class State { SENDING, FAILED }
    enum class Failure { SEND_FAILED, UPLOAD_FAILED, TOO_LARGE, UNSUPPORTED_TYPE }
}

/**
 * Sends messages and attachments in the app's scope instead of a screen's, so
 * leaving the chat right after "Send" loses nothing (AND-B-3).
 *
 * - A message is [PendingMessage.State.SENDING] until the server confirms it
 *   (it is then in [ChatRepository]'s list and leaves this one), or
 *   [PendingMessage.State.FAILED]: it stays visible until the user retries or
 *   discards it, also after leaving and reopening the chat.
 * - Per channel the sends run one after another (a [Mutex]), so messages keep
 *   the order they were sent in.
 * - [retry] only restarts a failed message; a second tap while it is sending
 *   does nothing, so nothing is sent twice.
 * - Sessions: every message remembers the session it was sent in. It is
 *   checked against the current session right before it goes out, and the
 *   request itself is bound to it (`RequiredSessionId`), so a message of
 *   account A never goes out with the credentials of account B, nor after
 *   signing out and in again as A. When a session ends ([AuthState.endedSessions],
 *   never conflated) and when its caches are cleared ([ChatRepository.clearSession],
 *   session teardown), its messages are cancelled and dropped with its drafts.
 *
 * Also holds the unsent draft per channel for this process (AND-B-19).
 */
@Singleton
class OutgoingMessages internal constructor(
    private val chatRepository: ChatRepository,
    private val authState: AuthState,
    private val scope: CoroutineScope,
    private val newId: () -> String,
) {

    @Inject
    constructor(
        chatRepository: ChatRepository,
        authState: AuthState,
        @ChatCoroutineScope scope: CoroutineScope,
    ) : this(chatRepository, authState, scope, { UUID.randomUUID().toString() })

    private companion object {
        val logger: Logger = Logger.getLogger("OutgoingMessages")
    }

    /** Guards [items] transitions together with [jobs]. */
    private val lock = Any()

    private val items = MutableStateFlow<List<PendingMessage>>(emptyList())
    private val jobs = HashMap<String, Job>()
    private val channelLocks = ConcurrentHashMap<String, Mutex>()
    /** Draft text per channel, with the session it was typed in. */
    private val drafts = ConcurrentHashMap<String, Pair<Long?, String>>()

    private val _failures = MutableSharedFlow<PendingMessage>(extraBufferCapacity = 16)

    /** Each message whose attempt just failed (for a snackbar on the open chat). */
    val failures: SharedFlow<PendingMessage> = _failures.asSharedFlow()

    init {
        // Explicit teardown path: session teardown clears the chat caches.
        chatRepository.addSessionClearListener { dropAllExcept(currentSessionId()) }
        // Every ended session, also logout + login of the same account in quick succession.
        scope.launch(start = CoroutineStart.UNDISPATCHED) {
            authState.endedSessions.collect { ended -> dropSession(ended.sessionId) }
        }
    }

    private fun currentSessionId(): Long? = (authState.session.value as? AuthSession.LoggedIn)?.sessionId

    /** Pending and failed messages of [channelId], oldest first. */
    fun pending(channelId: String): Flow<List<PendingMessage>> =
        items.map { all -> all.filter { it.channelId == channelId } }.distinctUntilChanged()

    /** Current pending messages of [channelId] (snapshot). */
    fun pendingNow(channelId: String): List<PendingMessage> =
        items.value.filter { it.channelId == channelId }

    /** The unsent draft of [channelId] in the current session. */
    fun draft(channelId: String): String =
        drafts[channelId]?.takeIf { it.first == currentSessionId() }?.second.orEmpty()

    fun setDraft(channelId: String, text: String) {
        if (text.isEmpty()) drafts.remove(channelId) else drafts[channelId] = currentSessionId() to text
    }

    /** Queues [text] for [channelId]; returns the pending message's local id. */
    fun send(channelId: String, text: String): String =
        enqueue(PendingMessage(newId(), channelId, text, sessionId = currentSessionId()))

    /** Queues the photo/video [uri] with an optional [caption]; returns the local id. */
    fun sendAttachment(channelId: String, uri: Uri, caption: String?): String =
        enqueue(
            PendingMessage(newId(), channelId, caption.orEmpty(), attachment = uri, sessionId = currentSessionId())
        )

    /** Sends a failed message again. Ignored unless it is [PendingMessage.State.FAILED]. */
    fun retry(localId: String) {
        val item = synchronized(lock) {
            val current = items.value.firstOrNull { it.localId == localId } ?: return
            if (current.state != PendingMessage.State.FAILED || jobs.containsKey(localId)) return
            val sending = current.copy(state = PendingMessage.State.SENDING, failure = null)
            replace(sending)
            sending
        }
        start(item)
    }

    /** Retries every failed message of [channelId], in order. */
    fun retryAll(channelId: String) {
        pendingNow(channelId)
            .filter { it.state == PendingMessage.State.FAILED }
            .forEach { retry(it.localId) }
    }

    /**
     * Drops a failed message and returns it (the screen may put its text back
     * into the field). A message that is still sending is not touched (null).
     */
    fun discard(localId: String): PendingMessage? = synchronized(lock) {
        val current = items.value.firstOrNull { it.localId == localId } ?: return null
        if (current.state != PendingMessage.State.FAILED) return null
        items.value = items.value.filterNot { it.localId == localId }
        current
    }

    /** Cancels every send and drops all pending messages and drafts. */
    fun clear() = drop { true }

    /** Drops everything of the ended session [sessionId]. */
    fun dropSession(sessionId: Long) = drop { it == sessionId }

    /** Drops everything that does not belong to [sessionId] (the current one, or null when signed out). */
    fun dropAllExcept(sessionId: Long?) = drop { it == null || it != sessionId }

    private fun drop(ofSession: (Long?) -> Boolean) {
        val running = synchronized(lock) {
            val gone = items.value.filter { ofSession(it.sessionId) }
            items.value = items.value - gone.toSet()
            gone.mapNotNull { jobs.remove(it.localId) }
        }
        drafts.entries.removeIf { ofSession(it.value.first) }
        running.forEach { it.cancel() }
    }

    private fun enqueue(item: PendingMessage): String {
        if (item.sessionId == null) {
            // Not signed in: nothing can be sent; show it as not sent instead of losing it.
            val failed = item.copy(state = PendingMessage.State.FAILED, failure = PendingMessage.Failure.SEND_FAILED)
            synchronized(lock) { items.value = items.value + failed }
            _failures.tryEmit(failed)
            return item.localId
        }
        synchronized(lock) { items.value = items.value + item }
        start(item)
        return item.localId
    }

    private fun replace(item: PendingMessage) {
        items.value = items.value.map { if (it.localId == item.localId) item else it }
    }

    private fun start(item: PendingMessage) {
        val job = scope.launch(start = CoroutineStart.LAZY) { deliver(item) }
        val started = synchronized(lock) {
            // Cleared in the meantime (sign-out): do not start.
            if (items.value.none { it.localId == item.localId }) {
                false
            } else {
                jobs[item.localId] = job
                true
            }
        }
        if (started) job.start() else job.cancel()
    }

    private suspend fun deliver(item: PendingMessage) {
        var outcome: Outcome = Outcome.Dropped
        try {
            channelLocks.getOrPut(item.channelId) { Mutex() }.withLock {
                // Checked right before it goes out: never in another session.
                if (item.sessionId == null || item.sessionId != currentSessionId()) {
                    outcome = Outcome.Dropped
                    return@withLock
                }
                outcome = if (item.attachment == null) {
                    chatRepository.sendMessage(item.channelId, item.text, item.sessionId)
                    Outcome.Sent
                } else {
                    val error = chatRepository.sendAttachment(
                        item.channelId, item.attachment, item.text.ifBlank { null }, item.sessionId,
                    ).exceptionOrNull()
                    when {
                        error == null -> Outcome.Sent
                        error.isSessionChange() -> Outcome.Dropped
                        else -> Outcome.Failed(uploadFailure(error))
                    }
                }
            }
        } catch (e: CancellationException) {
            // Only a real cancellation of this send (sign-out, clear) drops it silently;
            // a CancellationException thrown by a lower layer is an ordinary failure.
            if (!currentCoroutineContext().isActive) throw e
            logger.warning("send ended by a stray cancellation")
            outcome = Outcome.Failed(failureFor(item))
        } catch (e: Exception) {
            outcome = if (e.isSessionChange()) {
                Outcome.Dropped
            } else {
                logger.log(Level.WARNING, "Failed to send message (${e::class.simpleName})")
                Outcome.Failed(failureFor(item))
            }
        } finally {
            finish(item, outcome)
        }
    }

    private fun failureFor(item: PendingMessage) =
        if (item.attachment == null) PendingMessage.Failure.SEND_FAILED else PendingMessage.Failure.UPLOAD_FAILED

    private fun Throwable.isSessionChange(): Boolean =
        generateSequence(this) { it.cause }.any { it is SessionChangedException }

    private sealed interface Outcome {
        data object Sent : Outcome
        data object Dropped : Outcome
        data class Failed(val failure: PendingMessage.Failure) : Outcome
    }

    private fun finish(item: PendingMessage, outcome: Outcome) {
        val failed = synchronized(lock) {
            jobs.remove(item.localId)
            when {
                // Sent (now in ChatRepository's list), or its session is gone: never show it again.
                outcome !is Outcome.Failed -> {
                    items.value = items.value.filterNot { it.localId == item.localId }
                    null
                }
                items.value.none { it.localId == item.localId } -> null
                else -> item.copy(state = PendingMessage.State.FAILED, failure = (outcome as Outcome.Failed).failure).also(::replace)
            }
        }
        failed?.let { _failures.tryEmit(it) }
    }

    private fun uploadFailure(e: Throwable): PendingMessage.Failure = when (e) {
        is ChatRepository.AttachmentException.TooLarge -> PendingMessage.Failure.TOO_LARGE
        is ChatRepository.AttachmentException.UnsupportedType -> PendingMessage.Failure.UNSUPPORTED_TYPE
        else -> PendingMessage.Failure.UPLOAD_FAILED
    }
}
