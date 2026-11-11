package io.wolftown.kaiku.data.repository

import android.net.Uri
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.di.ChatCoroutineScope
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.Job
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
 * - When the signed-in user changes or signs out, every send is cancelled and
 *   all pending messages and drafts are dropped: nothing of one session is
 *   sent or shown in another.
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
    private val drafts = ConcurrentHashMap<String, String>()

    private val _failures = MutableSharedFlow<PendingMessage>(extraBufferCapacity = 16)

    /** Each message whose attempt just failed (for a snackbar on the open chat). */
    val failures: SharedFlow<PendingMessage> = _failures.asSharedFlow()

    /** Signed-in user the pending messages belong to. */
    @Volatile
    private var owner: String? = authState.currentUserId.value

    init {
        scope.launch(start = CoroutineStart.UNDISPATCHED) {
            authState.currentUserId.collect { user ->
                if (user != owner) {
                    owner = user
                    clear()
                }
            }
        }
    }

    /** Pending and failed messages of [channelId], oldest first. */
    fun pending(channelId: String): Flow<List<PendingMessage>> =
        items.map { all -> all.filter { it.channelId == channelId } }.distinctUntilChanged()

    /** Current pending messages of [channelId] (snapshot). */
    fun pendingNow(channelId: String): List<PendingMessage> =
        items.value.filter { it.channelId == channelId }

    fun draft(channelId: String): String = drafts[channelId].orEmpty()

    fun setDraft(channelId: String, text: String) {
        if (text.isEmpty()) drafts.remove(channelId) else drafts[channelId] = text
    }

    /** Queues [text] for [channelId]; returns the pending message's local id. */
    fun send(channelId: String, text: String): String =
        enqueue(PendingMessage(newId(), channelId, text))

    /** Queues the photo/video [uri] with an optional [caption]; returns the local id. */
    fun sendAttachment(channelId: String, uri: Uri, caption: String?): String =
        enqueue(PendingMessage(newId(), channelId, caption.orEmpty(), attachment = uri))

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

    /** Drops a failed message. A message that is still sending is not touched. */
    fun discard(localId: String) {
        synchronized(lock) {
            val current = items.value.firstOrNull { it.localId == localId } ?: return
            if (current.state != PendingMessage.State.FAILED) return
            items.value = items.value.filterNot { it.localId == localId }
        }
    }

    /** Cancels every send and drops all pending messages and drafts. */
    fun clear() {
        val running = synchronized(lock) {
            val all = jobs.values.toList()
            jobs.clear()
            items.value = emptyList()
            all
        }
        drafts.clear()
        running.forEach { it.cancel() }
    }

    private fun enqueue(item: PendingMessage): String {
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
        var failure: PendingMessage.Failure? = null
        var done = false
        try {
            channelLocks.getOrPut(item.channelId) { Mutex() }.withLock {
                failure = if (item.attachment == null) {
                    chatRepository.sendMessage(item.channelId, item.text)
                    null
                } else {
                    chatRepository.sendAttachment(item.channelId, item.attachment, item.text.ifBlank { null })
                        .exceptionOrNull()
                        ?.let(::uploadFailure)
                }
            }
            done = true
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            logger.log(Level.WARNING, "Failed to send message (${e::class.simpleName})")
            failure = PendingMessage.Failure.SEND_FAILED
            done = true
        } finally {
            finish(item, if (done) failure else null, cancelled = !done)
        }
    }

    private fun finish(item: PendingMessage, failure: PendingMessage.Failure?, cancelled: Boolean) {
        val failed = synchronized(lock) {
            jobs.remove(item.localId)
            when {
                // Session ended (ChatRepository cancels uploads) or clear(): never show it again.
                cancelled || failure == null -> {
                    items.value = items.value.filterNot { it.localId == item.localId }
                    null
                }
                items.value.none { it.localId == item.localId } -> null
                else -> item.copy(state = PendingMessage.State.FAILED, failure = failure).also(::replace)
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
