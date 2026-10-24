package io.wolftown.kaiku.push

import io.ktor.http.HttpStatusCode
import io.wolftown.kaiku.data.api.ApiException
import io.wolftown.kaiku.data.api.MessageApi
import io.wolftown.kaiku.data.api.UnreadApi
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

    /** New messages were found and recorded, but notifications are disabled. */
    NOTIFICATIONS_OFF,
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
 * Wakes and reads are unordered (C20), so whether anything notifies is
 * decided by the server's unread count for the channel: at most the newest
 * N messages (N = unread count) can notify, none when N is 0. The local read
 * cursor and seen watermark only filter further.
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
    private val unreadApi: UnreadApi,
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

    }

    /** The session a wake is processed for. */
    private data class Account(val userId: String, val generation: Long)

    /** The server's unread state of one channel. */
    private data class ServerUnread(val count: Int, val countsOwnMessages: Boolean)

    /**
     * [isStopped] reports that the work was stopped (e.g. a read cancelled it);
     * it is checked once more right before posting.
     */
    suspend fun process(signal: WakeSignal, owner: WakeOwner, isStopped: () -> Boolean = { false }): WakeOutcome {
        // Captured before the refresh: a refresh rotation keeps the generation,
        // a logout or new login changes it (TokenStorage).
        val generation = tokenStorage.getSessionGeneration()
        val userId = tokenStorage.getUserId()
        if (userId == null || tokenStorage.getRefreshToken() == null) {
            // No session at all: nothing may stay from the previous one.
            clearAll()
            return WakeOutcome.NOT_LOGGED_IN
        }
        if (owner.userId != userId || owner.serverUrl != tokenStorage.getServerUrl()) {
            return WakeOutcome.WRONG_ACCOUNT
        }
        val account = Account(userId, generation)

        when (refresher.ensureFresh(MIN_TOKEN_VALIDITY_MS)) {
            RefreshResult.SUCCESS -> Unit
            RefreshResult.NETWORK_ERROR -> return WakeOutcome.RETRY
            RefreshResult.AUTH_REJECTED, RefreshResult.NO_SESSION -> return sessionLost(account)
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
                // The HTTP client clears the tokens before it surfaces a real
                // rejection (401, 403 PASSWORD_CHANGE_REQUIRED): the session ended.
                !isCurrent(account) -> sessionLost(account)
                // Lost access to the channel: retrying cannot help.
                e.status == HttpStatusCode.Forbidden || e.status == HttpStatusCode.NotFound -> WakeOutcome.NOTHING_NEW
                // Includes a 401 with the session intact: transient, try again.
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
        if (newest == null || notifiable(account, channelId, unseen, unread = null).isEmpty()) {
            newest?.let { markSeen(account, channelId, it) }
            return finish(signal, account, WakeOutcome.NOTHING_NEW)
        }

        // The server's read state is authoritative (C20): a late wake never
        // re-notifies what was read, whatever arrived first.
        val unread = serverUnread(channelId)
        if (!isCurrent(account)) return sessionLost(account)
        if (notifiable(account, channelId, unseen, unread).isEmpty()) {
            markSeen(account, channelId, newest)
            return finish(signal, account, WakeOutcome.NOTHING_NEW)
        }

        val title = titles.resolve(channelId, preferDm = signal.kind == WakeKind.DM)

        // Final checks inside the presenter's critical section, right before posting:
        // a logout (cancelAll) or read (ledger write, then cancel) is either seen
        // here or removes the notification after it was posted.
        var refused: WakeOutcome = WakeOutcome.NOTHING_NEW
        val result = sink.show {
            val lines = notifiable(account, channelId, unseen, unread)
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

        val outcome = when (result) {
            ShowResult.REFUSED -> when (refused) {
                WakeOutcome.NOT_LOGGED_IN -> return sessionLost(account)
                WakeOutcome.SUPPRESSED_ACTIVE -> {
                    // The user opened the chat while we were fetching.
                    markSeen(account, channelId, newest)
                    catchUpQuietly(channelId)
                    return refused
                }
                else -> return refused
            }
            ShowResult.POSTED -> {
                // The presenter's lock is not TokenStorage's: a logout or read can
                // land between the check and the post. Re-check and take it back.
                if (!isCurrent(account)) {
                    sink.cancel(channelId)
                    return sessionLost(account)
                }
                if (notifiable(account, channelId, unseen, unread).isEmpty()) {
                    sink.cancel(channelId)
                    WakeOutcome.NOTHING_NEW
                } else {
                    WakeOutcome.NOTIFIED
                }
            }
            // Same bookkeeping as a post, just nothing shown.
            ShowResult.DISABLED -> WakeOutcome.NOTIFICATIONS_OFF
        }
        markSeen(account, channelId, newest)
        catchUpQuietly(channelId)
        return finish(signal, account, outcome)
    }

    /**
     * Others' messages of [unseen] that may notify: within the server's
     * newest-[ServerUnread.count] window (when [unread] is known), after the
     * re-read seen watermark and after the re-read local read cursor.
     */
    private fun notifiable(
        account: Account,
        channelId: String,
        unseen: List<Message>,
        unread: ServerUnread?,
    ): List<Message> {
        val fromOthers = { m: Message -> m.author.id != account.userId }
        val window: Set<String>? = unread?.let { u ->
            // Guild counts include own messages, DM counts do not.
            val candidates = if (u.countsOwnMessages) unseen else unseen.filter(fromOthers)
            candidates.takeLast(u.count).mapTo(HashSet()) { it.id }
        }
        val afterSeen = afterLastSeen(
            unseen,
            ledger.lastSeen(account.userId, channelId),
            ledger.lastSeenCreatedAt(account.userId, channelId),
        )
        return ledger.readCursor(account.userId, channelId)
            .unread(afterSeen)
            .filter { fromOthers(it) && (window == null || it.id in window) }
    }

    /** The server's unread count for [channelId]; a channel not listed has none. */
    private suspend fun serverUnread(channelId: String): ServerUnread {
        val aggregate = unreadApi.getUnread()
        aggregate.dms.firstOrNull { it.channelId == channelId }?.let {
            return ServerUnread(it.unreadCount.toCount(), countsOwnMessages = false)
        }
        aggregate.guilds.asSequence().flatMap { it.channels }.firstOrNull { it.channelId == channelId }?.let {
            return ServerUnread(it.unreadCount.toCount(), countsOwnMessages = true)
        }
        return ServerUnread(0, countsOwnMessages = false)
    }

    private fun Long.toCount(): Int = coerceIn(0L, Int.MAX_VALUE.toLong()).toInt()

    /** A read known only by id becomes exact once the fetch shows its `created_at`. */
    private fun resolveReadCursor(account: Account, channelId: String, unseen: List<Message>) {
        val message = ledger.readCursor(account.userId, channelId).resolve(unseen) ?: return
        if (!isCurrent(account)) return
        ledger.markRead(account.userId, channelId, message.id, message.createdAt)
    }

    /**
     * Last steps of a wake that found something: DM wakes reload the DM list.
     * That request can end the session too, which is handled like any other.
     */
    private suspend fun finish(signal: WakeSignal, account: Account, outcome: WakeOutcome): WakeOutcome {
        refreshDmList(signal)
        return if (isCurrent(account)) outcome else sessionLost(account)
    }

    /**
     * The wake's session is no longer usable. When it ended (no session
     * stored), its notifications and queued wakes go too, also when no app UI
     * is running to observe the logout. A newer session's state is never touched.
     */
    private fun sessionLost(account: Account): WakeOutcome {
        val ended = tokenStorage.getRefreshToken() == null || isCurrent(account)
        if (ended) clearAll()
        return WakeOutcome.NOT_LOGGED_IN
    }

    private fun clearAll() {
        logger.info("wake fetch: no session, clearing message notifications")
        sink.cancelAll()
        enqueuer.cancelAll()
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
