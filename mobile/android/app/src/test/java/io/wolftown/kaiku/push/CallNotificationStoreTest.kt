package io.wolftown.kaiku.push

import io.wolftown.kaiku.testutil.InMemorySharedPreferences
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class CallNotificationStoreTest {

    private var clock = 1_000_000L
    private val store = PrefsCallNotificationStore(InMemorySharedPreferences()) { clock }

    @Test
    fun `stores call id, phase and time per account and channel`() {
        store.setShown("me", "c1", ShownCall("call-1", CallPhase.RINGING, 42L))

        assertEquals(ShownCall("call-1", CallPhase.RINGING, 42L), store.shown("me", "c1"))
        assertNull(store.shown("me", "c2"))
        assertNull(store.shown("other", "c1"))
    }

    @Test
    fun `ended tombstone expires after its lifetime`() {
        store.setShown("me", "c1", ShownCall("call-1", CallPhase.ENDED, clock))

        clock += PrefsCallNotificationStore.TOMBSTONE_TTL_MS
        assertEquals(CallPhase.ENDED, store.shown("me", "c1")?.phase)

        clock += 1
        assertNull(store.shown("me", "c1"))
    }

    @Test
    fun `missed and ringing entries do not expire`() {
        store.setShown("me", "c1", ShownCall("call-1", CallPhase.MISSED, clock))
        store.setShown("me", "c2", ShownCall("call-2", CallPhase.RINGING, clock))

        clock += 10 * PrefsCallNotificationStore.TOMBSTONE_TTL_MS

        assertEquals(CallPhase.MISSED, store.shown("me", "c1")?.phase)
        assertEquals(CallPhase.RINGING, store.shown("me", "c2")?.phase)
    }

    @Test
    fun `clearUser keeps other accounts`() {
        store.setShown("me", "c1", ShownCall("call-1", CallPhase.RINGING, 0L))
        store.setShown("other", "c1", ShownCall("call-2", CallPhase.RINGING, 0L))

        store.clearUser("me")

        assertNull(store.shown("me", "c1"))
        assertEquals("call-2", store.shown("other", "c1")?.callId)
    }
}
