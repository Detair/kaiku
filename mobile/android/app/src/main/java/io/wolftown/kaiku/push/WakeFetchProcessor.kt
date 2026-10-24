package io.wolftown.kaiku.push

import io.ktor.http.HttpStatusCode
import io.wolftown.kaiku.data.api.ApiException
import io.wolftown.kaiku.data.api.MessageApi
import io.wolftown.kaiku.data.auth.RefreshResult
import io.wolftown.kaiku.data.auth.TokenRefresher
import io.wolftown.kaiku.data.local.TokenStorage
import io.wolftown.kaiku.data.repository.ChatRepository
import io.wolftown.kaiku.data.repository.DmRepository
import io.wolftown.kaiku.domain.MessageOrdering
import io.wolftown.kaiku.domain.model.Message
import kotlinx.coroutines.CancellationException
import java.util.logging.Logger
import javax.inject.Inject

/** Result of handling one wake signal. */
enum class WakeOutcome { NOTIFIED, NOTHING_NEW, SUPPRESSED_ACTIVE, NOT_LOGGED_IN, RETRY }

/**
 * Handles a DM/mention wake: makes sure the session's access token is usable,
 * fetches the channel's new messages over REST, posts a notification for the
 * messages of other people, and warms the chat cache.
 *
 * Push is only a hint (FCM may drop wakes, C20 note): correctness comes from
 * the catch-up on app start/reconnect ([io.wolftown.kaiku.data.sync.ResyncCoordinator]).
 * This only covers the woken channel and, for DM wakes, the DM list.
 *
 * Never logs tokens or message content.
 */
class WakeFetchProcessor @Inject constructor(
    private val refresher: TokenRefresher,
    private val tokenStorage: TokenStorage,
    private val messages: MessageApi,
    private val titles: NotificationTitleResolver,
    private val ledger: NotificationLedger,
    private val sink: NotificationSink,
    private val active: ActiveChannelTracker,
    private val chat: ChatRepository,
    private val dms: DmRepository,
) {
    companion object {
        private val logger = Logger.getLogger("WakeFetchProcessor")

        /** The access token must stay valid at least this long for the fetch. */
        const val MIN_TOKEN_VALIDITY_MS = 60_000L

        /** Page size of the `after=` fetch. */
        const val AFTER_PAGE_SIZE = 50

        /** Upper bound of the latest-page fetch (no or unusable last-seen id). */
        const val LATEST_PAGE_MAX = 10

        /** Lines kept in one channel notification. */
        const val MAX_LINES = 7
    }

    suspend fun process(signal: WakeSignal): WakeOutcome {
        if (tokenStorage.getRefreshToken() == null) return WakeOutcome.NOT_LOGGED_IN
        when (refresher.ensureFresh(MIN_TOKEN_VALIDITY_MS)) {
            RefreshResult.SUCCESS -> Unit
            RefreshResult.NETWORK_ERROR -> return WakeOutcome.RETRY
            RefreshResult.AUTH_REJECTED, RefreshResult.NO_SESSION -> return WakeOutcome.NOT_LOGGED_IN
        }
        // The session this wake is processed for; a logout or account switch
        // while fetching must not post anything.
        val session = tokenStorage.getSessionGeneration()
        val channelId = signal.channelId

        if (isVisible(channelId)) {
            catchUpQuietly(channelId)
            return WakeOutcome.SUPPRESSED_ACTIVE
        }

        return try {
            fetchAndNotify(signal, session)
        } catch (e: CancellationException) {
            throw e
        } catch (e: ApiException) {
            // Lost access to the channel: retrying cannot help.
            if (e.status == HttpStatusCode.Forbidden || e.status == HttpStatusCode.NotFound) {
                WakeOutcome.NOTHING_NEW
            } else {
                WakeOutcome.RETRY
            }
        } catch (_: Exception) {
            WakeOutcome.RETRY
        }
    }

    private suspend fun fetchAndNotify(signal: WakeSignal, session: Long): WakeOutcome {
        val channelId = signal.channelId
        val ownUserId = tokenStorage.getUserId()
        val lastSeen = ledger.lastSeen(channelId)
        val lastSeenAt = ledger.lastSeenCreatedAt(channelId)

        val sorted = MessageOrdering.merge(emptyList(), fetch(channelId, lastSeen, signal.count))
        val unseen = afterLastSeen(sorted, lastSeen, lastSeenAt)
        val newest = unseen.lastOrNull()
        val fromOthers = unseen.filter { it.author.id != ownUserId }

        if (newest == null || fromOthers.isEmpty()) {
            // Own messages are seen by definition: move the anchor past them.
            newest?.let { markSeen(channelId, it) }
            refreshDmList(signal)
            return WakeOutcome.NOTHING_NEW
        }

        val title = titles.resolve(channelId, preferDm = signal.kind == WakeKind.DM)

        if (tokenStorage.getSessionGeneration() != session || tokenStorage.getRefreshToken() == null) {
            return WakeOutcome.NOT_LOGGED_IN
        }
        if (isVisible(channelId)) {
            // The user opened the chat while we were fetching.
            markSeen(channelId, newest)
            catchUpQuietly(channelId)
            return WakeOutcome.SUPPRESSED_ACTIVE
        }

        sink.show(
            ChannelNotification(
                channelId = channelId,
                title = title?.first,
                isDm = title?.second ?: (signal.kind == WakeKind.DM),
                kind = signal.kind,
                lines = fromOthers.takeLast(MAX_LINES).map(::line),
            )
        )
        markSeen(channelId, newest)
        catchUpQuietly(channelId)
        refreshDmList(signal)
        return WakeOutcome.NOTIFIED
    }

    /**
     * With a last-seen id: one `after=` page. If the server reports more, or
     * rejects the anchor (400: message gone), the latest page is used instead,
     * so the notification shows the newest messages. Without one: the latest
     * page sized by the signal's count.
     */
    private suspend fun fetch(channelId: String, lastSeen: String?, count: Int): List<Message> {
        val latestLimit = count.coerceIn(1, LATEST_PAGE_MAX)
        if (lastSeen == null) return messages.getMessages(channelId, limit = latestLimit).items

        val page = try {
            messages.getMessages(channelId, after = lastSeen, limit = AFTER_PAGE_SIZE)
        } catch (e: ApiException) {
            if (e.status != HttpStatusCode.BadRequest) throw e
            logger.info("wake fetch: anchor rejected (400), using latest page")
            return messages.getMessages(channelId, limit = latestLimit).items
        }
        if (!page.hasMore) return page.items
        return messages.getMessages(channelId, limit = LATEST_PAGE_MAX).items
    }

    /** Drops messages at or before the last-seen position ([sorted] is oldest first). */
    private fun afterLastSeen(sorted: List<Message>, lastSeen: String?, lastSeenAt: String?): List<Message> {
        if (lastSeen == null) return sorted
        val index = sorted.indexOfFirst { it.id == lastSeen }
        val tail = if (index >= 0) sorted.drop(index + 1) else sorted
        if (lastSeenAt == null) return tail
        return tail.filter { MessagePosition.isAfter(it.createdAt, it.id, lastSeenAt, lastSeen) != false }
    }

    private fun markSeen(channelId: String, message: Message) {
        ledger.markSeen(channelId, message.id, message.createdAt.ifEmpty { null })
    }

    private fun line(m: Message) = NotificationLine(
        messageId = m.id,
        senderName = m.author.displayName.ifBlank { m.author.username },
        text = NotificationText.forMessage(m),
        timestampMs = MessagePosition.parse(m.createdAt)?.toEpochMilli() ?: System.currentTimeMillis(),
    )

    private fun isVisible(channelId: String) = active.visibleChannelId.value == channelId

    private suspend fun catchUpQuietly(channelId: String) {
        try {
            chat.catchUp(channelId)
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            logger.info("wake fetch: cache catch-up failed (${e::class.simpleName})")
        }
    }

    /** DM wakes also reload the DM list, so the other conversations' unread badges are right. */
    private suspend fun refreshDmList(signal: WakeSignal) {
        if (signal.kind != WakeKind.DM) return
        try {
            dms.loadDms()
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            logger.info("wake fetch: DM list reload failed (${e::class.simpleName})")
        }
    }
}
