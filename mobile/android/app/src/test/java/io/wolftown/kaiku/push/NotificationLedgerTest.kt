package io.wolftown.kaiku.push

import io.wolftown.kaiku.domain.model.Message
import io.wolftown.kaiku.domain.model.User
import io.wolftown.kaiku.testutil.InMemorySharedPreferences
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class NotificationLedgerTest {

    private val ledger = PrefsNotificationLedger(InMemorySharedPreferences())

    // Random (UUIDv4) ids: their sort order says nothing about time.
    private val late = "0a000000-0000-4000-8000-000000000000"
    private val early = "ff000000-0000-4000-8000-000000000000"

    @Test
    fun `position only moves forward by created_at not by id`() {
        ledger.markSeen("me", "c1", late, "2026-10-24T10:05:00Z")
        ledger.markSeen("me", "c1", early, "2026-10-24T10:01:00Z")

        assertEquals(late, ledger.lastSeen("me", "c1"))
        assertEquals("2026-10-24T10:05:00Z", ledger.lastSeenCreatedAt("me", "c1"))

        val later = "55000000-0000-4000-8000-000000000000"
        ledger.markSeen("me", "c1", later, "2026-10-24T10:06:00Z")
        assertEquals(later, ledger.lastSeen("me", "c1"))
    }

    @Test
    fun `equal created_at is broken by id`() {
        val at = "2026-10-24T10:05:00Z"
        ledger.markSeen("me", "c1", "bb000000-0000-4000-8000-000000000000", at)
        ledger.markSeen("me", "c1", "aa000000-0000-4000-8000-000000000000", at)
        assertEquals("bb000000-0000-4000-8000-000000000000", ledger.lastSeen("me", "c1"))

        ledger.markSeen("me", "c1", "cc000000-0000-4000-8000-000000000000", at)
        assertEquals("cc000000-0000-4000-8000-000000000000", ledger.lastSeen("me", "c1"))
    }

    @Test
    fun `unknown position never replaces a timestamped one`() {
        ledger.markSeen("me", "c1", late, "2026-10-24T10:05:00Z")
        ledger.markSeen("me", "c1", early, null)
        assertEquals(late, ledger.lastSeen("me", "c1"))
    }

    @Test
    fun `timestamped position replaces an unknown one`() {
        ledger.markSeen("me", "c1", early, null)
        ledger.markSeen("me", "c1", late, "2026-10-24T10:05:00Z")
        assertEquals(late, ledger.lastSeen("me", "c1"))
        assertEquals("2026-10-24T10:05:00Z", ledger.lastSeenCreatedAt("me", "c1"))
    }

    @Test
    fun `entries are kept per user`() {
        ledger.markSeen("alice", "c1", late, "2026-10-24T10:05:00Z")

        assertNull(ledger.lastSeen("bob", "c1"))
        assertEquals(late, ledger.lastSeen("alice", "c1"))

        ledger.clear("alice", "c1")
        assertNull(ledger.lastSeen("alice", "c1"))
        assertNull(ledger.lastSeenCreatedAt("alice", "c1"))
    }

    // -- Read cursor -----------------------------------------------------------

    private fun m(id: String, minute: Int) = Message(
        id = id,
        channelId = "c1",
        author = User(id = "bob", username = "bob", displayName = "Bob"),
        content = "",
        createdAt = "2026-10-24T10:%02d:00Z".format(minute),
    )

    @Test
    fun `exact read cursor only moves forward`() {
        ledger.markRead("me", "c1", late, "2026-10-24T10:05:00Z")
        ledger.markRead("me", "c1", early, "2026-10-24T10:01:00Z")

        assertEquals(ReadCursor(id = late, at = "2026-10-24T10:05:00Z"), ledger.readCursor("me", "c1"))
    }

    @Test
    fun `exact read cursor covers messages up to it`() {
        val cursor = ReadCursor(id = "b", at = "2026-10-24T10:05:00Z")
        val unread = cursor.unread(listOf(m("x", 4), m("a", 5), m("b", 5), m("c", 5), m("y", 6)))
        assertEquals(listOf("c", "y"), unread.map { it.id })
    }

    @Test
    fun `id-only read is the boundary when fetched`() {
        val cursor = ReadCursor(receivedId = "m5")
        val unread = cursor.unread((2..6).map { m("m$it", it) })
        assertEquals(listOf("m6"), unread.map { it.id })
    }

    @Test
    fun `id-only read that is not fetched filters nothing`() {
        val cursor = ReadCursor(receivedId = "gone")
        val unread = cursor.unread(listOf(m("m2", 2), m("m40", 40)))
        assertEquals(listOf("m2", "m40"), unread.map { it.id })
    }

    @Test
    fun `resolving the id-only read makes it exact`() {
        ledger.markRead("me", "c1", "m5", null)
        assertEquals(ReadCursor(receivedId = "m5"), ledger.readCursor("me", "c1"))

        ledger.markRead("me", "c1", "m5", "2026-10-24T10:05:00Z")
        assertEquals(ReadCursor(id = "m5", at = "2026-10-24T10:05:00Z"), ledger.readCursor("me", "c1"))
    }

    @Test
    fun `read cursor survives a new ledger instance on the same storage`() {
        val prefs = InMemorySharedPreferences()
        PrefsNotificationLedger(prefs).markRead("me", "c1", "m5", "2026-10-24T10:05:00Z")

        assertEquals("m5", PrefsNotificationLedger(prefs).readCursor("me", "c1").id)
        assertEquals(ReadCursor(), PrefsNotificationLedger(prefs).readCursor("other", "c1"))
    }
}
