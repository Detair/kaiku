package io.wolftown.kaiku.data.sync

import io.wolftown.kaiku.data.repository.ChatRepository
import io.wolftown.kaiku.data.repository.DmRepository
import io.wolftown.kaiku.data.repository.GuildRepository
import io.wolftown.kaiku.data.ws.KaikuWebSocket
import io.wolftown.kaiku.data.ws.ServerEvent
import io.wolftown.kaiku.di.ChatCoroutineScope
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.joinAll
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeoutOrNull
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.contentOrNull
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger
import java.util.logging.Logger
import javax.inject.Inject
import javax.inject.Singleton

/**
 * Brings client state back in line with the server after every (re)connect.
 *
 * A new socket starts with no channel subscriptions, and messages posted
 * while offline were never delivered. On every `Ready` this:
 *
 * 1. re-sends `subscribe` for every open channel and background DM
 *    ([ChatRepository.resubscribeAll]);
 * 2. catches up each of those channels **after its `subscribed` ack**
 *    ([ChatRepository.catchUp]; it skips channels whose list was never
 *    loaded). Fetching before the ack can lose messages posted between the
 *    fetch and the subscription (gap-free order in `server/src/ws/AGENTS.md`).
 *    An older server that never acks falls back to catching up after
 *    [SUBSCRIBED_TIMEOUT_MS];
 * 3. reloads the selected server's channels (unread counts)
 *    ([GuildRepository.reloadSelectedGuildChannels]) once this Ready's
 *    catch-ups are done (bounded by [CATCH_UP_SETTLE_TIMEOUT_MS]), so the
 *    catch-up's per-message unread increments do not add to the server counts;
 * 4. reloads the DM list and subscribes to every DM channel
 *    ([DmRepository.loadDms], [ChatRepository.setDmChannels]). `dm_created`
 *    auto-subscribes only sockets open at creation, so this is the only way a
 *    reconnected socket gets DM messages live.
 *
 * Each step runs in its own `runCatching` and is logged, so one failure does
 * not abort the others. A `dm_created` event registers the new DM as a
 * background subscription ([ChatRepository.trackDmChannel]).
 */
@Singleton
class ResyncCoordinator @Inject constructor(
    private val webSocket: KaikuWebSocket,
    private val chat: ChatRepository,
    private val guilds: GuildRepository,
    private val dms: DmRepository,
    @ChatCoroutineScope private val scope: CoroutineScope,
) {
    companion object {
        private val logger = Logger.getLogger("ResyncCoordinator")

        /** How long to wait for a `subscribed` ack before catching up anyway (old servers). */
        const val SUBSCRIBED_TIMEOUT_MS = 5_000L

        /**
         * Upper bound for the channel reload to wait for this Ready's catch-ups:
         * the ack wait plus a budget for the catch-up fetches.
         */
        const val CATCH_UP_SETTLE_TIMEOUT_MS = SUBSCRIBED_TIMEOUT_MS + 10_000L

        /** How often the channel reload checks whether acks are still pending. */
        private const val ACK_POLL_MS = 100L
    }

    private val started = AtomicBoolean(false)

    /** Incremented on every Ready; stale timers and acks of an older socket are ignored. */
    private val generation = AtomicInteger(0)

    /** Channels waiting for their `subscribed` ack before catch-up, with the Ready generation. */
    private val awaitingAck = ConcurrentHashMap<String, Int>()

    @Volatile
    private var resyncJobs: List<Job> = emptyList()

    /** Catch-up jobs of the current generation; the channel reload waits for them. */
    @Volatile
    private var catchUpJobs = CatchUpJobs(generation = 0)

    /**
     * Guards "ack consumed + catch-up job recorded" against the channel
     * reload's "nothing pending" check, so the reload never sees a channel
     * that is neither awaiting its ack nor recorded as a catch-up job.
     */
    private val catchUpLock = Any()

    /** Catch-up jobs of one generation; guarded by [catchUpLock]. */
    private class CatchUpJobs(val generation: Int) {
        val jobs = mutableListOf<Job>()
    }

    /** Starts observing server events; idempotent. */
    fun start() {
        if (!started.compareAndSet(false, true)) return
        // UNDISPATCHED: subscribe to the replay-0 SharedFlow before start() returns.
        scope.launch(start = CoroutineStart.UNDISPATCHED) {
            webSocket.events.collect { event ->
                when (event) {
                    is ServerEvent.Ready -> onReady()
                    is ServerEvent.Subscribed -> onSubscribed(event.channelId)
                    is ServerEvent.DmCreated -> onDmCreated(event)
                    else -> Unit
                }
            }
        }
    }

    /**
     * Session teardown: stops the running resync (reloads, ack waits,
     * catch-ups) so nothing of the ended session is fetched or cached again.
     * The event collector keeps running; the next `Ready` starts afresh.
     */
    fun cancelForLogout() {
        val gen = generation.incrementAndGet()
        awaitingAck.clear()
        resyncJobs.forEach { it.cancel() }
        resyncJobs = emptyList()
        val previousJobs = synchronized(catchUpLock) {
            val previous = catchUpJobs.jobs.toList()
            catchUpJobs = CatchUpJobs(gen)
            previous
        }
        previousJobs.forEach { it.cancel() }
    }

    /**
     * Runs on the collector, so the `subscribe`s are sent and registered
     * before any `subscribed` ack of the new socket is handled.
     */
    private fun onReady() {
        val gen = generation.incrementAndGet()
        awaitingAck.clear()
        resyncJobs.forEach { it.cancel() }
        val catchUps = CatchUpJobs(gen)
        val previousJobs = synchronized(catchUpLock) {
            val previous = catchUpJobs.jobs.toList()
            catchUpJobs = catchUps
            previous
        }
        previousJobs.forEach { it.cancel() }

        // DMs tracked from dm_created after this point may be missing from the
        // DM list fetched below; setDmChannels must not unsubscribe them.
        val dmMark = chat.dmTrackingMark()
        val resubscribed = step("resubscribe") { chat.resubscribeAll() }.orEmpty()
        awaitAck(resubscribed, gen)

        // The channel reload replaces local unread counts with the server's,
        // which already include the missed messages. It runs after the
        // catch-ups (bounded), so their per-message increments cannot land on
        // top of the server counts.
        val reloadChannels = scope.launch {
            withTimeoutOrNull(CATCH_UP_SETTLE_TIMEOUT_MS) { awaitCatchUps(catchUps) }
            step("reload channels") { guilds.reloadSelectedGuildChannels() }
        }
        val reloadDms = scope.launch {
            step("reload dms") {
                dms.loadDms()
                if (gen != generation.get()) return@step
                val ids = dms.conversations.value.map { it.id }
                // Register before sending so a fast ack cannot be missed.
                awaitAck(ids.filterNot(chat::isDmChannelTracked), gen)
                chat.setDmChannels(ids, keepTrackedAfter = dmMark)
            }
        }
        resyncJobs = listOf(reloadChannels, reloadDms)
    }

    /** Suspends until no channel of this generation awaits its ack and all its catch-ups finished. */
    private suspend fun awaitCatchUps(catchUps: CatchUpJobs) {
        while (true) {
            val (pending, jobs) = synchronized(catchUpLock) {
                awaitingAck.values.any { it == catchUps.generation } to catchUps.jobs.toList()
            }
            if (pending) {
                // Acks or the fallback start the remaining catch-ups.
                delay(ACK_POLL_MS)
                continue
            }
            if (jobs.all { it.isCompleted }) return
            jobs.joinAll()
        }
    }

    private fun awaitAck(channelIds: Collection<String>, gen: Int) {
        if (channelIds.isEmpty()) return
        channelIds.forEach { awaitingAck[it] = gen }
        // Fallback for servers that never send `subscribed`.
        scope.launch {
            delay(SUBSCRIBED_TIMEOUT_MS)
            if (gen != generation.get()) return@launch
            channelIds.forEach { id ->
                if (startCatchUpIfAwaiting(id, gen)) {
                    logger.info("no subscribed ack within ${SUBSCRIBED_TIMEOUT_MS}ms, caught up anyway")
                }
            }
        }
    }

    private fun onSubscribed(channelId: String) {
        val gen = awaitingAck[channelId] ?: return
        startCatchUpIfAwaiting(channelId, gen)
    }

    /**
     * Consumes the ack wait of [channelId] for generation [gen] and starts its
     * catch-up, once. Returns whether a catch-up was started.
     */
    private fun startCatchUpIfAwaiting(channelId: String, gen: Int): Boolean {
        val job = synchronized(catchUpLock) {
            val catchUps = catchUpJobs
            if (gen != generation.get() || catchUps.generation != gen) return false
            if (!awaitingAck.remove(channelId, gen)) return false
            scope.launch(start = CoroutineStart.LAZY) { step("catch up") { chat.catchUp(channelId) } }
                .also { catchUps.jobs += it }
        }
        job.start()
        return true
    }

    private fun onDmCreated(event: ServerEvent.DmCreated) {
        // A malformed event must not throw: that would end the collector for good.
        val id = (event.channel["id"] as? JsonPrimitive)?.contentOrNull ?: return
        chat.trackDmChannel(id)
    }

    /** Runs one resync step; a failure is logged (type only) and does not propagate. */
    private inline fun <T> step(name: String, block: () -> T): T? =
        try {
            block()
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            logger.warning("resync step '$name' failed: ${e::class.simpleName}")
            null
        }
}
