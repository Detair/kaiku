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
    )

    private fun store() = PrefsKeyDeviceStore(KeystoreWrappedPreferences(backing, testValueCipher()))

    @Test fun `nothing stored is absent`() {
        assertEquals(KeyStorage.Absent, store().load())
    }

    @Test fun `a stored key device reads back with its key, setup and claim time`() {
        assertTrue(store().save(record))
        assertEquals(KeyStorage.Present(record), store().load())
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
