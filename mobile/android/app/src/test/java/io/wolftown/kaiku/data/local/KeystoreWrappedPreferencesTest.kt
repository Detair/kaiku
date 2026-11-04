package io.wolftown.kaiku.data.local

import io.wolftown.kaiku.testutil.InMemorySharedPreferences
import io.wolftown.kaiku.testutil.testValueCipher
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.security.GeneralSecurityException
import java.util.Base64

class KeystoreWrappedPreferencesTest {

    private val backing = InMemorySharedPreferences()
    private val prefs = KeystoreWrappedPreferences(backing, testValueCipher())

    @Test fun `values round trip through the cipher`() {
        prefs.edit().putString("access_token", "tok").putLong("expires_at", 1234L).putBoolean("flag", true).commit()
        assertEquals("tok", prefs.getString("access_token", null))
        assertEquals(1234L, prefs.getLong("expires_at", 0L))
        assertTrue(prefs.getBoolean("flag", false))
        assertNull(prefs.getString("missing", null))
        assertEquals(7L, prefs.getLong("missing", 7L))
        assertEquals(mapOf("access_token" to "tok", "expires_at" to 1234L, "flag" to true), prefs.all)
        prefs.edit().remove("access_token").commit()
        assertFalse(prefs.contains("access_token"))
    }

    @Test fun `stored bytes are not the plain value`() {
        prefs.edit().putString("refresh_token", "refresh-secret-value").commit()
        val stored = backing.getString("refresh_token", null)!!
        assertFalse(stored.contains("refresh-secret-value"))
        val raw = Base64.getDecoder().decode(stored)
        assertFalse(String(raw, Charsets.ISO_8859_1).contains("refresh-secret-value"))
        // Fresh IV per write: the same value encrypts differently.
        prefs.edit().putString("refresh_token", "refresh-secret-value").commit()
        assertFalse(stored == backing.getString("refresh_token", null))
    }

    @Test fun `a value moved to another key name fails to decrypt`() {
        prefs.edit().putString("refresh_token", "r").commit()
        backing.edit().putString("access_token", backing.getString("refresh_token", null)).commit()
        assertThrowsSecurity { prefs.getString("access_token", null) }
    }

    @Test fun `damaged blob makes TokenStorage reset and report once`() {
        val storage = TokenStorage(prefs, InMemorySharedPreferences())
        storage.saveServerUrl("https://chat.example.com")
        storage.saveTokens("access-1", "refresh-1", 900, "user-1")
        backing.edit().putString("access_token", Base64.getEncoder().encodeToString(ByteArray(40) { 1 })).commit()

        assertNull(storage.getAccessToken())
        assertNull(storage.getRefreshToken())
        assertEquals("https://chat.example.com", storage.getServerUrl())
        assertTrue(storage.consumeStorageResetNotice())
        assertFalse(storage.consumeStorageResetNotice())
    }

    private fun assertThrowsSecurity(block: () -> Unit) {
        try {
            block()
        } catch (_: GeneralSecurityException) {
            return
        }
        throw AssertionError("expected GeneralSecurityException")
    }
}
