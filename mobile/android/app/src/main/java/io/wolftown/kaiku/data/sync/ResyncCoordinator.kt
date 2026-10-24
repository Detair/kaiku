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
import kotlinx.coroutines.launch
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.jsonPrimitive
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
 *    ([GuildRepository.reloadSelectedGuildChannels]);
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
    }

    private val started = AtomicBoolean(false)

    /** Incremented on every Ready; stale timers and acks of an older socket are ignored. */
    private val generation = AtomicInteger(0)

    /** Channels waiting for their `subscribed` ack before catch-up, with the Ready generation. */
    private val awaitingAck = ConcurrentHashMap<String, Int>()

    private var resyncJob: Job? = null

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
     * Runs on the collector, so the `subscribe`s are sent and registered
     * before any `subscribed` ack of the new socket is handled.
     */
    private fun onReady() {
        val gen = generation.incrementAndGet()
        awaitingAck.clear()
        resyncJob?.cancel()

        val resubscribed = step("resubscribe") { chat.resubscribeAll() }.orEmpty()
        awaitAck(resubscribed, gen)

        resyncJob = scope.launch {
            step("reload channels") { guilds.reloadSelectedGuildChannels() }
            step("reload dms") {
                dms.loadDms()
                val ids = dms.conversations.value.map { it.id }
                // Register before sending so a fast ack cannot be missed.
                awaitAck(ids.filterNot(chat::isDmChannelTracked), gen)
                chat.setDmChannels(ids)
            }
        }
    }

    private fun awaitAck(channelIds: Collection<String>, gen: Int) {
        if (channelIds.isEmpty()) return
        channelIds.forEach { awaitingAck[it] = gen }
        // Fallback for servers that never send `subscribed`.
        scope.launch {
            delay(SUBSCRIBED_TIMEOUT_MS)
            channelIds.forEach { id ->
                if (awaitingAck.remove(id, gen)) {
                    logger.info("no subscribed ack within ${SUBSCRIBED_TIMEOUT_MS}ms, catching up anyway")
                    launchCatchUp(id)
                }
            }
        }
    }

    private fun onSubscribed(channelId: String) {
        val gen = awaitingAck[channelId] ?: return
        if (gen == generation.get() && awaitingAck.remove(channelId, gen)) launchCatchUp(channelId)
    }

    private fun launchCatchUp(channelId: String) {
        scope.launch { step("catch up") { chat.catchUp(channelId) } }
    }

    private fun onDmCreated(event: ServerEvent.DmCreated) {
        val id = event.channel["id"]?.jsonPrimitive?.contentOrNull ?: return
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
