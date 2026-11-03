package io.wolftown.kaiku.data.local

import android.content.SharedPreferences
import io.wolftown.kaiku.testutil.InMemorySharedPreferences
import io.wolftown.kaiku.testutil.testValueCipher
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.security.GeneralSecurityException

class LegacyTokenMigrationTest {

    private val target = KeystoreWrappedPreferences(InMemorySharedPreferences(), testValueCipher())

    @Test fun `migration copies tokens and deletes the legacy store`() {
        val legacy = InMemorySharedPreferences()
        legacy.edit()
            .putString("access_token", "a").putString("refresh_token", "r").putLong("expires_at", 99L)
            .putString("user_id", "u").putString("server_url", "https://chat.example.com")
            .putString("unrelated", "x")
            .commit()
        var deleted = false

        val ok = LegacyTokenMigration.run(
            legacyExists = true,
            openLegacy = { legacy },
            deleteLegacy = { deleted = true },
            target = target,
            keys = TokenStorage.STORED_KEYS,
        )

        assertTrue(ok)
        assertTrue(deleted)
        val storage = TokenStorage(target)
        assertEquals("a", storage.getAccessToken())
        assertEquals("r", storage.getRefreshToken())
        assertEquals(99L, storage.getAccessTokenExpiresAtMillis())
        assertEquals("u", storage.getUserId())
        assertEquals("https://chat.example.com", storage.getServerUrl())
        assertNull(target.getString("unrelated", null))
    }

    @Test fun `no legacy store does nothing`() {
        var opened = false
        assertTrue(LegacyTokenMigration.run(false, { opened = true; null }, {}, target, TokenStorage.STORED_KEYS))
        assertFalse(opened)
    }

    @Test fun `damaged legacy store is dropped without crashing`() {
        var deleted = false
        // Cannot be opened at all (key lost after a restore).
        assertFalse(LegacyTokenMigration.run(true, { throw GeneralSecurityException("x") }, { deleted = true }, target, TokenStorage.STORED_KEYS))
        assertTrue(deleted)
        // Opens, but reading fails.
        deleted = false
        val unreadable = object : SharedPreferences by InMemorySharedPreferences() {
            override fun getAll(): MutableMap<String, *> = throw SecurityException("x")
        }
        assertFalse(LegacyTokenMigration.migrate(unreadable, target, TokenStorage.STORED_KEYS))
        assertFalse(LegacyTokenMigration.run(true, { unreadable }, { deleted = true }, target, TokenStorage.STORED_KEYS))
        assertTrue(deleted)
        assertNull(TokenStorage(target).getAccessToken())
    }
}
