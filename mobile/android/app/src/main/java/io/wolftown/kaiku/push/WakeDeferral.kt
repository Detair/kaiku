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
 * count; no content) until the storage can be read and the session is known again, and then go
 * through the normal queue ([release]); with nobody signed in by then they are dropped. A wake
 * whose account was already known (a queued fetch that met the closed storage) keeps that account
 * and goes out only for it. Call wakes are not kept: a ring is over long before.
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

    /** Keeps [signal] (one per channel, the higher count) until [release]; [owner]: the account it was queued for, if known. */
    @Synchronized
    fun hold(signal: WakeSignal, owner: WakeOwner? = null) {
        if (signal.kind == WakeKind.CALL) return
        val all = read()
        val before = all.firstOrNull { it.signal.channelId == signal.channelId }
        val kept = all.filterNot { it.signal.channelId == signal.channelId }.toMutableList()
        kept += Held(signal.copy(count = maxOf(signal.count, before?.signal?.count ?: 0)), owner)
        write(kept.takeLast(MAX))
    }

    /** Hands every kept wake, with its account if one was kept, to [enqueue] once and forgets them. */
    @Synchronized
    fun release(enqueue: (WakeSignal, WakeOwner?) -> Unit) {
        val kept = read()
        if (kept.isEmpty()) return
        write(emptyList())
        kept.forEach { enqueue(it.signal, it.owner) }
    }

    private data class Held(val signal: WakeSignal, val owner: WakeOwner?)

    /** Forgets every kept wake (nobody is signed in to receive them). */
    @Synchronized
    fun clear() = write(emptyList())

    private fun read(): List<Held> =
        prefs.getString(KEY, null).orEmpty().lines().filter { it.isNotBlank() }.mapNotNull { line ->
            val parts = line.split('|')
            if (parts.size != 3 && parts.size != 5) return@mapNotNull null
            val kind = WakeKind.entries.firstOrNull { it.name == parts[0] } ?: return@mapNotNull null
            val owner = if (parts.size == 5) {
                WakeOwner(decode(parts[3]) ?: return@mapNotNull null, parts[4].takeIf { it.isNotEmpty() }?.let { decode(it) ?: return@mapNotNull null })
            } else {
                null
            }
            Held(WakeSignal(kind, parts[1], parts[2].toIntOrNull() ?: 1), owner)
        }

    private fun write(held: List<Held>) {
        prefs.edit().putString(KEY, held.joinToString("\n") { h ->
            val s = h.signal
            "${s.kind.name}|${s.channelId}|${s.count}" + (h.owner?.let { "|${encode(it.userId)}|${it.serverUrl?.let(::encode).orEmpty()}" } ?: "")
        }).commit()
    }

    private fun encode(v: String): String =
        java.util.Base64.getUrlEncoder().withoutPadding().encodeToString(v.toByteArray(Charsets.UTF_8))

    private fun decode(v: String): String? =
        runCatching { String(java.util.Base64.getUrlDecoder().decode(v), Charsets.UTF_8) }.getOrNull()
}
