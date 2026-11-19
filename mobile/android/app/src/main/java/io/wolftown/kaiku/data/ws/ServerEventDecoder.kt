package io.wolftown.kaiku.data.ws

import kotlinx.serialization.ExperimentalSerializationApi
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.atomic.AtomicLong
import java.util.logging.Level
import java.util.logging.Logger

/**
 * Decodes server frames into [ServerEvent]s. Event types this app does not
 * model (the server sends e.g. `rich_presence_update` and
 * `custom_status_update` to every client) are a quiet, counted skip: debug
 * log once per type. Only a frame of a known type that does not decode is a
 * warning, since that is a real contract break.
 */
class ServerEventDecoder(private val json: Json) {

    /** The result of one frame. */
    sealed interface Result {
        data class Event(val event: ServerEvent) : Result
        data class Skipped(val type: String) : Result
        data class Broken(val type: String, val error: Exception) : Result
    }

    private val skipped = ConcurrentHashMap<String, AtomicLong>()

    /** How many frames of each unmodelled type were skipped (diagnostics, tests). */
    fun skippedCounts(): Map<String, Long> = skipped.mapValues { it.value.get() }

    fun decode(text: String): Result {
        val element = try {
            json.parseToJsonElement(text) as? JsonObject
        } catch (e: Exception) {
            return Result.Broken("unparseable", e)
        } ?: return Result.Broken("unknown", IllegalArgumentException("frame is not an object"))
        // Only a JSON string is a type; a number or a boolean is a broken frame.
        val type = (element["type"] as? JsonPrimitive)?.takeIf { it.isString }?.content
            ?: return Result.Broken("unknown", IllegalArgumentException("frame without a string type"))
        if (type !in KNOWN_TYPES) {
            val count = skipped.computeIfAbsent(type) { AtomicLong() }.incrementAndGet()
            if (count == 1L) logger.log(Level.FINE, "skipping unmodelled server event type=$type")
            return Result.Skipped(type)
        }
        return try {
            // From the element already parsed: each frame is parsed once.
            Result.Event(json.decodeFromJsonElement(ServerEvent.serializer(), element))
        } catch (e: Exception) {
            Result.Broken(type, e)
        }
    }

    companion object {
        private val logger = Logger.getLogger("ServerEventDecoder")

        /** The `type` names of every [ServerEvent] subclass, from the sealed serializer. */
        @OptIn(ExperimentalSerializationApi::class)
        val KNOWN_TYPES: Set<String> = ServerEvent.serializer().descriptor.getElementDescriptor(1)
            .let { d -> (0 until d.elementsCount).mapTo(HashSet()) { d.getElementName(it) } }
    }
}
