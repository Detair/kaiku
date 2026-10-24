package io.wolftown.kaiku.push

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
}
