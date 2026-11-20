package io.wolftown.kaiku.push

import android.annotation.SuppressLint
import android.content.Context
import android.content.SharedPreferences
import dagger.hilt.android.qualifiers.ApplicationContext
import javax.inject.Inject
import javax.inject.Singleton

/**
 * Message wakes that arrived while the key storage could not be read: nothing about the stored
 * session could be read then, not even whose account it is. They are kept (channel, kind and
 * count; no content, no account) until the storage can be read and the session is known again,
 * and then go through the normal queue for that account ([release]); with nobody signed in by then
 * they are dropped. Call wakes are not kept: a ring is over long before.
 */
@Singleton
// Written at once: the process that keeps a wake may end right after.
@SuppressLint("ApplySharedPref")
class WakeDeferral internal constructor(private val prefs: SharedPreferences) {

    @Inject
    constructor(@ApplicationContext context: Context) : this(context.getSharedPreferences(PREFS, Context.MODE_PRIVATE))

    private companion object {
        const val PREFS = "wake_deferred"
        const val KEY = "signals"
        /** At most this many channels are kept; the oldest go first. */
        const val MAX = 50
    }

    /** Keeps [signal] (one per channel, the higher count) until [release]. */
    @Synchronized
    fun hold(signal: WakeSignal) {
        if (signal.kind == WakeKind.CALL) return
        val kept = read().filterNot { it.channelId == signal.channelId }.toMutableList()
        val before = read().firstOrNull { it.channelId == signal.channelId }
        kept += signal.copy(count = maxOf(signal.count, before?.count ?: 0))
        write(kept.takeLast(MAX))
    }

    /** Hands every kept wake to [enqueue] once and forgets them. */
    @Synchronized
    fun release(enqueue: (WakeSignal) -> Unit) {
        val kept = read()
        if (kept.isEmpty()) return
        write(emptyList())
        kept.forEach(enqueue)
    }

    /** Forgets every kept wake (nobody is signed in to receive them). */
    @Synchronized
    fun clear() = write(emptyList())

    private fun read(): List<WakeSignal> =
        prefs.getString(KEY, null).orEmpty().lines().filter { it.isNotBlank() }.mapNotNull { line ->
            val parts = line.split('|')
            if (parts.size != 3) return@mapNotNull null
            val kind = WakeKind.entries.firstOrNull { it.name == parts[0] } ?: return@mapNotNull null
            WakeSignal(kind, parts[1], parts[2].toIntOrNull() ?: 1)
        }

    private fun write(signals: List<WakeSignal>) {
        prefs.edit().putString(KEY, signals.joinToString("\n") { "${it.kind.name}|${it.channelId}|${it.count}" }).commit()
    }
}
