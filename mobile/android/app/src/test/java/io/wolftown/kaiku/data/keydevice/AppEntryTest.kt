package io.wolftown.kaiku.data.keydevice

import io.wolftown.kaiku.approval.UvClass
import io.wolftown.kaiku.data.local.AuthSession
import org.junit.Assert.assertEquals
import org.junit.Test
import java.time.Instant

/** Where the phone belongs, for every storage state, stored phase and session. */
class AppEntryTest {

    private fun record(phase: KeyDevicePhase) = StoredKeyDevice(
        LocalKeyDevice(null, "u1", "https://chat.example.com", UvClass.PER_USE, "e1", null),
        phase,
        readyAt = if (phase == KeyDevicePhase.PENDING) Instant.parse("2026-11-21T10:00:00Z") else null,
    )

    @Test fun `the one table of where the phone belongs`() {
        val signedIn = AuthSession.LoggedIn("u1", 1)
        val signedInOther = AuthSession.LoggedIn("u2", 2)
        val phases: List<KeyDevicePhase?> = listOf(null) + KeyDevicePhase.entries
        for (session in listOf(AuthSession.LoggedOut, signedIn, signedInOther)) {
            for (phase in phases) {
                for (hasServer in listOf(true, false)) {
                    val r = phase?.let(::record)
                    val case = "session=$session phase=$phase server=$hasServer"
                    // Unreadable decides before anything else, the record is not even looked at.
                    assertEquals(case, AppEntry.UNREADABLE, appEntry(true, r, session, hasServer))
                    val expected = when {
                        session is AuthSession.LoggedIn -> AppEntry.HOME
                        phase == KeyDevicePhase.CLAIMING || phase == KeyDevicePhase.CLAIMED || phase == KeyDevicePhase.PENDING -> AppEntry.WAITING_SETUP
                        phase == KeyDevicePhase.ACTIVE -> AppEntry.CONFIRM_ITS_YOU
                        !hasServer -> AppEntry.SERVER_ADDRESS
                        else -> AppEntry.SIGN_IN
                    }
                    assertEquals(case, expected, appEntry(false, r, session, hasServer))
                }
            }
        }
    }

    /** A waiting key device without its ready time still owns the entry (it is a stored setup all the same). */
    @Test fun `a waiting key device without its time still belongs to the waiting screen`() {
        val r = record(KeyDevicePhase.PENDING).copy(readyAt = null)
        assertEquals(AppEntry.WAITING_SETUP, appEntry(false, r, AuthSession.LoggedOut, true))
    }

    @Test fun `only a setup that has not finished owns the entry`() {
        assertEquals(false, setupOwnsEntry(null))
        assertEquals(true, setupOwnsEntry(record(KeyDevicePhase.CLAIMING)))
        assertEquals(true, setupOwnsEntry(record(KeyDevicePhase.CLAIMED)))
        assertEquals(true, setupOwnsEntry(record(KeyDevicePhase.PENDING)))
        assertEquals(false, setupOwnsEntry(record(KeyDevicePhase.ACTIVE)))
    }
}
