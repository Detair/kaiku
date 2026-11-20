package io.wolftown.kaiku.data.keydevice

import io.wolftown.kaiku.approval.UvClass
import io.wolftown.kaiku.data.local.KeystoreWrappedPreferences
import io.wolftown.kaiku.data.local.ValueCipher
import io.wolftown.kaiku.testutil.InMemorySharedPreferences
import io.wolftown.kaiku.testutil.testValueCipher
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.security.ProviderException
import java.time.Instant

/** A store that cannot be read is "unreadable", never "no key device", and nothing in it is touched. */
class PrefsKeyDeviceStoreTest {

    private val backing = InMemorySharedPreferences()
    private val device = LocalKeyDevice("kd1", "u1", "https://chat.example.com", UvClass.PER_USE, "e1", null, "Oma")
    private val record = StoredKeyDevice(
        device, KeyDevicePhase.CLAIMING, keyId = "key-1", attemptId = "attempt-1",
        claimVoidAfter = Instant.parse("2026-11-20T10:20:00Z"),
        installId = "22222222-2222-4222-8222-222222222222", since = Instant.parse("2026-11-20T10:00:00Z"),
        codeClaim = true,
        unanswered = Unanswered(Instant.parse("2026-11-20T11:00:00Z"), Instant.parse("2026-11-21T12:00:00Z"), 3),
    )

    private fun store() = PrefsKeyDeviceStore(KeystoreWrappedPreferences(backing, testValueCipher()))

    @Test fun `nothing stored is absent`() {
        assertEquals(KeyStorage.Absent, store().load())
    }

    @Test fun `a stored key device reads back with its key, setup, install id and times`() {
        assertTrue(store().save(record))
        assertEquals(KeyStorage.Present(record), store().load())
        assertEquals("22222222-2222-4222-8222-222222222222", (store().load() as KeyStorage.Present).record.installId)
        assertEquals(Instant.parse("2026-11-20T10:00:00Z"), (store().load() as KeyStorage.Present).record.since)
    }

    @Test fun `a record saved over one with an install id and a time does not keep the old ones`() {
        store().save(record)
        val plain = StoredKeyDevice(device, KeyDevicePhase.ACTIVE, keyId = "key-1")
        store().save(plain)
        assertEquals(KeyStorage.Present(plain), store().load())
    }

    @Test fun `the kind of claim and the notes about a silent server read back, and go with the record`() {
        store().save(record)
        val back = (store().load() as KeyStorage.Present).record
        assertTrue(back.codeClaim)
        assertEquals(Unanswered(Instant.parse("2026-11-20T11:00:00Z"), Instant.parse("2026-11-21T12:00:00Z"), 3), back.unanswered)
        // A record of an earlier version says neither.
        store().save(StoredKeyDevice(device, KeyDevicePhase.CLAIMING, keyId = "key-1"))
        val older = (store().load() as KeyStorage.Present).record
        assertFalse(older.codeClaim)
        assertNull(older.unanswered)
        // Cleared with the record: nothing of it is left for a later one.
        store().save(record)
        assertTrue(store().clear())
        assertEquals(KeyStorage.Absent, store().load())
        store().save(StoredKeyDevice(device, KeyDevicePhase.CLAIMING, keyId = "key-1"))
        assertNull((store().load() as KeyStorage.Present).record.unanswered)
    }

    @Test fun `notes about a silent server that are not whole make the record unreadable, not different`() {
        store().save(record)
        KeystoreWrappedPreferences(backing, testValueCipher()).edit().remove("unanswered_occasions").commit()
        assertEquals(KeyStorage.Unreadable, store().load())
        store().save(record)
        KeystoreWrappedPreferences(backing, testValueCipher()).edit().putString("unanswered_last", "yesterday").commit()
        assertEquals(KeyStorage.Unreadable, store().load())
    }

    @Test fun `a record of an earlier version reads back without a key id`() {
        store().save(StoredKeyDevice(device, KeyDevicePhase.ACTIVE))
        assertEquals(KeyStorage.Present(StoredKeyDevice(device, KeyDevicePhase.ACTIVE)), store().load())
    }

    @Test fun `a Keystore failure while reading is unreadable, and the file is left as it is`() {
        store().save(record)
        KeystoreWrappedPreferences(backing, testValueCipher()).edit().putBoolean("removal_sign_in", true).commit()
        val before = backing.all.toMap()
        val broken = object : ValueCipher {
            override fun encrypt(plain: ByteArray, aad: ByteArray) = throw ProviderException("keystore")
            override fun decrypt(blob: ByteArray, aad: ByteArray) = throw ProviderException("keystore")
        }
        val damaged = PrefsKeyDeviceStore(KeystoreWrappedPreferences(backing, broken))
        assertEquals(KeyStorage.Unreadable, damaged.load())
        assertNull(damaged.legacyRemovalMark())
        assertEquals(before, backing.all.toMap())
        // Readable again later: the record is still there.
        assertEquals(KeyStorage.Present(record), store().load())
    }

    @Test fun `a value of the wrong type is unreadable`() {
        val prefs = KeystoreWrappedPreferences(backing, testValueCipher())
        PrefsKeyDeviceStore(prefs).save(record)
        prefs.edit().putLong("phase", 3L).commit()
        assertEquals(KeyStorage.Unreadable, PrefsKeyDeviceStore(prefs).load())
    }

    @Test fun `an unreadable time is unreadable`() {
        val prefs = KeystoreWrappedPreferences(backing, testValueCipher())
        PrefsKeyDeviceStore(prefs).save(record)
        prefs.edit().putString("ready_at", "not a time").commit()
        assertEquals(KeyStorage.Unreadable, PrefsKeyDeviceStore(prefs).load())
    }

    @Test fun `a phase this version does not know, or a missing part, is unreadable`() {
        val prefs = KeystoreWrappedPreferences(backing, testValueCipher())
        PrefsKeyDeviceStore(prefs).save(record)
        prefs.edit().putString("phase", "SOMETHING_NEW").commit()
        assertEquals(KeyStorage.Unreadable, PrefsKeyDeviceStore(prefs).load())
        prefs.edit().putString("phase", "ACTIVE").remove("origin").commit()
        assertEquals(KeyStorage.Unreadable, PrefsKeyDeviceStore(prefs).load())
    }

    @Test fun `a write that does not reach storage says so`() {
        val broken = object : ValueCipher {
            override fun encrypt(plain: ByteArray, aad: ByteArray) = throw ProviderException("keystore")
            override fun decrypt(blob: ByteArray, aad: ByteArray) = throw ProviderException("keystore")
        }
        assertFalse(PrefsKeyDeviceStore(KeystoreWrappedPreferences(backing, broken)).save(record))
        assertEquals(KeyStorage.Absent, store().load())
    }

    @Test fun `clearing the record keeps a flag an earlier version left beside it`() {
        val prefs = KeystoreWrappedPreferences(backing, testValueCipher())
        PrefsKeyDeviceStore(prefs).save(record)
        prefs.edit().putBoolean("removal_sign_in", true).commit()
        val s = PrefsKeyDeviceStore(prefs)
        assertEquals(true, s.legacyRemovalMark())
        assertTrue(s.clear())
        assertEquals(KeyStorage.Absent, s.load())
        assertEquals(true, s.legacyRemovalMark())
        assertTrue(s.clearLegacyRemovalMark())
        assertEquals(false, s.legacyRemovalMark())
    }
}
