package io.wolftown.kaiku.data.keydevice

import io.wolftown.kaiku.approval.UvClass
import io.wolftown.kaiku.data.local.KeystoreWrappedPreferences
import io.wolftown.kaiku.data.local.ValueCipher
import io.wolftown.kaiku.testutil.InMemorySharedPreferences
import io.wolftown.kaiku.testutil.testValueCipher
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import java.security.ProviderException

/** A damaged key-device store reads as "no key device" and never crashes the app at start. */
class PrefsKeyDeviceStoreTest {

    private val backing = InMemorySharedPreferences()
    private val device = LocalKeyDevice("kd1", "u1", "https://chat.example.com", UvClass.PER_USE, "e1", null, "Oma")

    @Test fun `a stored key device reads back`() {
        val store = PrefsKeyDeviceStore(KeystoreWrappedPreferences(backing, testValueCipher()))
        store.save(StoredKeyDevice(device, KeyDevicePhase.ACTIVE))
        assertEquals(StoredKeyDevice(device, KeyDevicePhase.ACTIVE), store.load())
    }

    @Test fun `a Keystore failure while reading reads as no key device`() {
        PrefsKeyDeviceStore(KeystoreWrappedPreferences(backing, testValueCipher())).save(StoredKeyDevice(device, KeyDevicePhase.ACTIVE))
        val broken = object : ValueCipher {
            override fun encrypt(plain: ByteArray, aad: ByteArray) = throw ProviderException("keystore")
            override fun decrypt(blob: ByteArray, aad: ByteArray) = throw ProviderException("keystore")
        }
        assertNull(PrefsKeyDeviceStore(KeystoreWrappedPreferences(backing, broken)).load())
    }

    @Test fun `a value of the wrong type reads as no key device`() {
        val prefs = KeystoreWrappedPreferences(backing, testValueCipher())
        PrefsKeyDeviceStore(prefs).save(StoredKeyDevice(device, KeyDevicePhase.ACTIVE))
        prefs.edit().putLong("phase", 3L).commit()
        assertNull(PrefsKeyDeviceStore(prefs).load())
    }

    @Test fun `an unreadable time reads as no key device`() {
        val prefs = KeystoreWrappedPreferences(backing, testValueCipher())
        PrefsKeyDeviceStore(prefs).save(StoredKeyDevice(device, KeyDevicePhase.ACTIVE))
        prefs.edit().putString("ready_at", "not a time").commit()
        assertNull(PrefsKeyDeviceStore(prefs).load())
    }
}
