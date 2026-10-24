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
import io.wolftown.kaiku.domain.model.CursorPage
import io.wolftown.kaiku.domain.model.Message
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Job
import kotlinx.coroutines.currentCoroutineContext
import java.util.logging.Logger
import javax.inject.Inject

/** Result of handling one wake signal. */
enum class WakeOutcome {
    NOTIFIED,
    NOTHING_NEW,
    SUPPRESSED_ACTIVE,
    NOT_LOGGED_IN,

    /** The wake was queued for another account or server than the signed-in one. */
    WRONG_ACCOUNT,
    RETRY,
}

/**
 * Handles a DM/mention wake: makes sure the session's access token is usable,
 * fetches the channel's new messages over REST, posts a notification for the
 * messages of other people, and warms the chat cache.
 *
 * Everything is bound to the account and session the wake was queued for:
 * a logout or account change at any point means nothing is posted and the
 * ledger is not touched.
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
    private val enqueuer: WakeWorkEnqueuer,
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

        /** At most this many `after=` pages per wake; the next wake continues from there. */
        const val MAX_AFTER_PAGES = 5

        /** Upper bound of the latest-page fetch (no or unusable last-seen id). */
        const val LATEST_PAGE_MAX = 10

        /** Lines kept in one channel notification. */
        const val MAX_LINES = 7

        /** C4 error code: the account must change its password before using the API. */
        const val PASSWORD_CHANGE_REQUIRED = "PASSWORD_CHANGE_REQUIRED"
    }

    /** The session a wake is processed for. */
    private data class Account(val userId: String, val generation: Long)

    /**
     * [isStopped] reports that the work was stopped (e.g. a read cancelled it);
     * it is checked once more right before posting.
     */
    suspend fun process(signal: WakeSignal, owner: WakeOwner, isStopped: () -> Boolean = { false }): WakeOutcome {
        // Captured before the refresh: a refresh rotation keeps the generation,
        // a logout or new login changes it (TokenStorage).
        val generation = tokenStorage.getSessionGeneration()
        val userId = tokenStorage.getUserId() ?: return WakeOutcome.NOT_LOGGED_IN
        if (tokenStorage.getRefreshToken() == null) return WakeOutcome.NOT_LOGGED_IN
        if (owner.userId != userId || owner.serverUrl != tokenStorage.getServerUrl()) {
            return WakeOutcome.WRONG_ACCOUNT
        }
        val account = Account(userId, generation)

        when (refresher.ensureFresh(MIN_TOKEN_VALIDITY_MS)) {
            RefreshResult.SUCCESS -> Unit
            RefreshResult.NETWORK_ERROR -> return WakeOutcome.RETRY
            RefreshResult.AUTH_REJECTED, RefreshResult.NO_SESSION -> {
                onSessionEnded(account)
                return WakeOutcome.NOT_LOGGED_IN
            }
        }
        if (!isCurrent(account)) return sessionLost(account)

        val channelId = signal.channelId
        if (isVisible(channelId)) {
            catchUpQuietly(channelId)
            return WakeOutcome.SUPPRESSED_ACTIVE
        }

        val currentJob = currentCoroutineContext()[Job]
        val stopped = { isStopped() || currentJob?.isActive == false }
        return try {
            fetchAndNotify(signal, account, stopped)
        } catch (e: CancellationException) {
            throw e
        } catch (e: ApiException) {
            when {
                // The session can no longer fetch: rejected, or ended by the HTTP
                // client (401 after a failed refresh, 403 PASSWORD_CHANGE_REQUIRED).
                e.status == HttpStatusCode.Unauthorized || e.code == PASSWORD_CHANGE_REQUIRED ||
                    !isCurrent(account) -> sessionLost(account, force = true)
                // Lost access to the channel: retrying cannot help.
                e.status == HttpStatusCode.Forbidden || e.status == HttpStatusCode.NotFound -> WakeOutcome.NOTHING_NEW
                else -> WakeOutcome.RETRY
            }
        } catch (_: Exception) {
            if (isCurrent(account)) WakeOutcome.RETRY else sessionLost(account)
        }
    }

    private suspend fun fetchAndNotify(signal: WakeSignal, account: Account, stopped: () -> Boolean): WakeOutcome {
        val channelId = signal.channelId
        val lastSeen = ledger.lastSeen(account.userId, channelId)
        val lastSeenAt = ledger.lastSeenCreatedAt(account.userId, channelId)

        val unseen = fetchUnseen(channelId, lastSeen, lastSeenAt, signal.count)
        if (!isCurrent(account)) return sessionLost(account)
        resolveReadCursor(account, channelId, unseen)
        // Newest message actually considered; own messages count as seen.
        val newest = unseen.lastOrNull()
        val eligible = notifiable(account, channelId, unseen)

        if (newest == null || eligible.isEmpty()) {
            newest?.let { markSeen(account, channelId, it) }
            refreshDmList(signal)
            return WakeOutcome.NOTHING_NEW
        }

        val title = titles.resolve(channelId, preferDm = signal.kind == WakeKind.DM)

        // Final checks inside the presenter's critical section, right before posting:
        // a logout (cancelAll) or read (ledger write, then cancel) is either seen
        // here or removes the notification after it was posted.
        var refused: WakeOutcome = WakeOutcome.NOTHING_NEW
        val posted = sink.show {
            val lines = notifiable(account, channelId, unseen)
            when {
                stopped() -> null.also { refused = WakeOutcome.NOTHING_NEW }
                !isCurrent(account) -> null.also { refused = WakeOutcome.NOT_LOGGED_IN }
                isVisible(channelId) -> null.also { refused = WakeOutcome.SUPPRESSED_ACTIVE }
                lines.isEmpty() -> null.also { refused = WakeOutcome.NOTHING_NEW }
                else -> ChannelNotification(
                    userId = account.userId,
                    channelId = channelId,
                    title = title?.first,
                    isDm = title?.second ?: (signal.kind == WakeKind.DM),
                    kind = signal.kind,
                    lines = lines.takeLast(MAX_LINES).map(::line),
                )
            }
        }
        if (!posted) {
            if (refused == WakeOutcome.NOT_LOGGED_IN) return sessionLost(account)
            if (refused == WakeOutcome.SUPPRESSED_ACTIVE) {
                // The user opened the chat while we were fetching.
                markSeen(account, channelId, newest)
                catchUpQuietly(channelId)
            }
            return refused
        }
        markSeen(account, channelId, newest)
        catchUpQuietly(channelId)
        refreshDmList(signal)
        return WakeOutcome.NOTIFIED
    }

    /**
     * Others' messages of [unseen] that may notify: after the (re-read)
     * notified watermark and after the (re-read) read cursor. A wake may
     * arrive after the read of the same message (C20: unordered); the read wins.
     */
    private fun notifiable(account: Account, channelId: String, unseen: List<Message>): List<Message> {
        val afterSeen = afterLastSeen(
            unseen,
            ledger.lastSeen(account.userId, channelId),
            ledger.lastSeenCreatedAt(account.userId, channelId),
        )
        return ledger.readCursor(account.userId, channelId)
            .unread(afterSeen)
            .filter { it.author.id != account.userId }
    }

    /** A read known only by id becomes exact once the fetch shows its `created_at`. */
    private fun resolveReadCursor(account: Account, channelId: String, unseen: List<Message>) {
        val cursor = ledger.readCursor(account.userId, channelId)
        val message = cursor.resolve(unseen) ?: return
        ledger.markRead(account.userId, channelId, message.id, message.createdAt, cursor.receivedAt ?: message.createdAt)
    }

    /**
     * The session this wake ran for is gone: remove its notifications and
     * queued wakes, also when no app UI is running to observe the logout.
     * Skipped when another session took over meanwhile.
     */
    private fun onSessionEnded(account: Account) {
        val gone = tokenStorage.getRefreshToken() == null
        if (!gone && !isCurrent(account)) return
        logger.info("wake fetch: session ended, clearing message notifications")
        sink.cancelAll()
        enqueuer.cancelAll()
    }

    /**
     * The wake's session is no longer usable. Clears its notifications and
     * wakes when it ended (no session stored), or with [force] when the server
     * refused the session itself; a newer session's state is never touched.
     */
    private fun sessionLost(account: Account, force: Boolean = false): WakeOutcome {
        if (force || tokenStorage.getRefreshToken() == null) onSessionEnded(account)
        return WakeOutcome.NOT_LOGGED_IN
    }

    /**
     * The channel's messages after the last-seen position, oldest first.
     * With a last-seen id: `after=` pages following `next_cursor`, up to
     * [MAX_AFTER_PAGES]; a rejected anchor (400: message gone) falls back to
     * the latest page. Without one: the latest page sized by the signal's count.
     */
    private suspend fun fetchUnseen(
        channelId: String,
        lastSeen: String?,
        lastSeenAt: String?,
        count: Int,
    ): List<Message> {
        val latestLimit = count.coerceIn(1, LATEST_PAGE_MAX)
        if (lastSeen == null) {
            return MessageOrdering.merge(emptyList(), messages.getMessages(channelId, limit = latestLimit).items)
        }

        var page: CursorPage<Message> = try {
            messages.getMessages(channelId, after = lastSeen, limit = AFTER_PAGE_SIZE)
        } catch (e: ApiException) {
            if (e.status != HttpStatusCode.BadRequest) throw e
            logger.info("wake fetch: anchor rejected (400), using latest page")
            val latest = messages.getMessages(channelId, limit = latestLimit).items
            return afterLastSeen(MessageOrdering.merge(emptyList(), latest), lastSeen, lastSeenAt)
        }
        val collected = page.items.toMutableList()
        var pages = 1
        while (page.hasMore && pages < MAX_AFTER_PAGES) {
            val next = page.nextCursor ?: break
            page = messages.getMessages(channelId, after = next, limit = AFTER_PAGE_SIZE)
            collected += page.items
            pages++
        }
        return afterLastSeen(MessageOrdering.merge(emptyList(), collected), lastSeen, lastSeenAt)
    }

    /** Drops messages at or before the last-seen position ([sorted] is oldest first). */
    private fun afterLastSeen(sorted: List<Message>, lastSeen: String?, lastSeenAt: String?): List<Message> {
        if (lastSeen == null) return sorted
        val index = sorted.indexOfFirst { it.id == lastSeen }
        val tail = if (index >= 0) sorted.drop(index + 1) else sorted
        if (lastSeenAt == null) return tail
        return tail.filter { MessagePosition.isAfter(it.createdAt, it.id, lastSeenAt, lastSeen) != false }
    }

    /** Ledger write bound to [account]; skipped once that session is gone. */
    private fun markSeen(account: Account, channelId: String, message: Message) {
        if (!isCurrent(account)) return
        ledger.markSeen(account.userId, channelId, message.id, message.createdAt.ifEmpty { null })
    }

    private fun isCurrent(account: Account): Boolean =
        tokenStorage.getSessionGeneration() == account.generation &&
            tokenStorage.getUserId() == account.userId &&
            tokenStorage.getRefreshToken() != null

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
